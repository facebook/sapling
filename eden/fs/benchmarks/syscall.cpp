/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Exception.h>
#include <folly/File.h>
#include <gflags/gflags.h>
#include <sys/xattr.h>
#include "eden/common/utils/benchharness/Bench.h"

namespace {

DEFINE_string(
    filename,
    "syscall.tmp",
    "Path which should be opened and repeatedly getxattr'd");

/*
 * Benchmark getxattr in particular because it's not cached. This makes it a
 * good proxy for syscall overhead.
 */
void call_getxattr(benchmark::State& state) {
  folly::File file{FLAGS_filename, O_CREAT | O_EXCL | O_RDONLY | O_CLOEXEC};

  char buf[1000];
  for (auto _ : state) {
    // We don't check for errors because EdenFS does not support arbitrary
    // xattrs.
#ifdef __APPLE__
    (void)::fgetxattr(file.fd(), "user.benchmark", buf, std::size(buf), 0, 0);
#else
    (void)::fgetxattr(file.fd(), "user.benchmark", buf, std::size(buf));
#endif
  }

  folly::checkUnixError(::unlink(FLAGS_filename.c_str()));
}
BENCHMARK(call_getxattr);

} // namespace

EDEN_BENCHMARK_MAIN();
