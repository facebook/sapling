/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>
#include <random>
#include "eden/common/utils/benchharness/Bench.h"

namespace {
constexpr size_t kPageSize = 4096;

DEFINE_string(
    filename,
    "random_writes.tmp",
    "Path to which writes should be issued");
DEFINE_uint64(filesize, kPageSize * 4096, "File size in bytes");

struct TemporaryFile {
  TemporaryFile()
      : file{FLAGS_filename, O_CREAT | O_EXCL | O_WRONLY | O_CLOEXEC} {
    if (FLAGS_filesize == 0 || (FLAGS_filesize % kPageSize)) {
      throw std::invalid_argument{"file size must be multiple of page size"};
    }
    folly::checkUnixError(ftruncate(file.fd(), FLAGS_filesize), "ftruncate");
  }

  ~TemporaryFile() {
    folly::checkUnixError(::unlink(FLAGS_filename.c_str()));
  }

  folly::File file;
};

int getTemporaryFD() {
  static TemporaryFile tf;
  return tf.file.fd();
}

void random_writes(benchmark::State& state) {
  int fd = getTemporaryFD();
  off_t pageCount = FLAGS_filesize / kPageSize;

  char pagebuf[kPageSize];

  {
    folly::File urandom{"/dev/urandom", O_RDONLY | O_CLOEXEC};
    folly::checkUnixError(
        folly::readFull(urandom.fd(), pagebuf, sizeof(pagebuf)),
        "read /dev/urandom");
  }

  std::default_random_engine gen{std::random_device{}()};
  std::uniform_int_distribution<off_t> rng{0, pageCount - 1};

  // std::uniform_int_distribution has as much userspace CPU cost as
  // __libc_pwrite64 so pregenerate some offsets.
  off_t offsets[16 * 1024];
  std::generate(std::begin(offsets), std::end(offsets), [&] {
    return rng(gen) * kPageSize;
  });

  size_t offset_index = 0;
  for (auto _ : state) {
    off_t offset = offsets[offset_index++ % std::size(offsets)];
    folly::checkUnixError(pwrite(fd, pagebuf, kPageSize, offset));
  }
}

void random_writes_no_cancellation(benchmark::State& state) {
  int oldstate;
  XCHECK_EQ(0, pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &oldstate));
  SCOPE_EXIT {
    pthread_setcancelstate(oldstate, &oldstate);
  };

  int oldtype;
  XCHECK_EQ(0, pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &oldtype));
  SCOPE_EXIT {
    pthread_setcanceltype(oldtype, &oldtype);
  };

  random_writes(state);
}

BENCHMARK(random_writes)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16);

BENCHMARK(random_writes_no_cancellation)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16);
} // namespace

EDEN_BENCHMARK_MAIN();
