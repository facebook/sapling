/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>
#include <algorithm>
#include <random>
#include "eden/common/utils/benchharness/Bench.h"

namespace {
constexpr size_t kPageSize = 4096;
constexpr size_t kDefaultFileSize = 16 * 1024 * 1024;

DEFINE_string(
    filename,
    "random_writes.tmp",
    "Path to which writes should be issued");
DEFINE_uint64(filesize, kDefaultFileSize, "File size in bytes");

struct TemporaryFile {
  TemporaryFile()
      : file{FLAGS_filename, O_CREAT | O_EXCL | O_WRONLY | O_CLOEXEC} {
    if (FLAGS_filesize == 0 || (FLAGS_filesize % kPageSize)) {
      throw std::invalid_argument{"file size must be multiple of page size"};
    }
    folly::checkUnixError(ftruncate(file.fd(), FLAGS_filesize), "ftruncate");
  }

  ~TemporaryFile() {
    file.close();
    if (-1 == unlink(FLAGS_filename.c_str())) {
      int err = errno;
      fmt::print(
          stderr,
          "error unlinking {}: {}",
          FLAGS_filename,
          folly::errnoStr(err));
    }
  }

  folly::File file;
};

int getTemporaryFD() {
  static TemporaryFile tf;
  return tf.file.fd();
}

using random_bytes_engine = std::independent_bits_engine<
    std::default_random_engine,
    CHAR_BIT,
    unsigned short>;

void random_writes(benchmark::State& state) {
  int fd = getTemporaryFD();
  off_t pageCount = FLAGS_filesize / kPageSize;

  uint8_t pagebuf[kPageSize];
  std::generate(std::begin(pagebuf), std::end(pagebuf), random_bytes_engine{});

  std::default_random_engine gen{std::random_device{}()};

  // std::uniform_int_distribution has as much userspace CPU cost as
  // __libc_pwrite64 so pregenerate some offsets.
  std::vector<off_t> offsets(pageCount);
  std::generate(offsets.begin(), offsets.end(), [offset = 0ull]() mutable {
    return (offset++) * kPageSize;
  });
  std::shuffle(offsets.begin(), offsets.end(), gen);

  size_t total_written = 0;
  size_t total_pages = 0;
  size_t offset_index = 0;
  for (auto _ : state) {
    off_t offset = offsets[offset_index];
    if (offset_index++ == offsets.size()) {
      // Redoing the offsets is okay
      total_pages += offsets.size();
      offset_index = 0;
    }
    int result = pwrite(fd, pagebuf, kPageSize, offset);
    folly::checkUnixError(result);
    if (result != kPageSize) {
      fmt::print(stderr, "write was not complete: {} != {}", result, kPageSize);
    }
    total_written += result;
  }
  total_pages += offset_index;

  state.SetItemsProcessed(total_pages);
  state.SetBytesProcessed(total_written);
}

BENCHMARK(random_writes)
    // By default, google benchmark shows throughput numbers in bytes per CPU
    // second. That's not useful, so tell it we care about wall clock time.
    ->UseRealTime()
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16);

#ifdef __GLIBC__

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

BENCHMARK(random_writes_no_cancellation)
    // By default, google benchmark shows throughput numbers in bytes per CPU
    // second. That's not useful, so tell it we care about wall clock time.
    ->UseRealTime()
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16);

#endif

} // namespace

EDEN_BENCHMARK_MAIN();
