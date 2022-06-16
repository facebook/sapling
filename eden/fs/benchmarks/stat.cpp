/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/portability/GFlags.h>
#include "eden/common/utils/benchharness/Bench.h"

namespace {

DEFINE_string(
    filename,
    "stat.tmp",
    "Path which should be opened and repeatedly stat'd");

void call_fstat(benchmark::State& state) {
  folly::File file{FLAGS_filename, O_CREAT | O_RDONLY | O_CLOEXEC};
  struct stat buf;

  for (auto _ : state) {
    folly::checkUnixError(::fstat(file.fd(), &buf), "fstat failed");
  }

  folly::checkUnixError(::unlink(FLAGS_filename.c_str()));
}
BENCHMARK(call_fstat);

} // namespace

EDEN_BENCHMARK_MAIN();
