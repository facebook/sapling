/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Try.h>
#include <folly/system/Pid.h>
#include "eden/common/os/ProcessId.h"
#include "eden/common/utils/benchharness/Bench.h"

#ifdef _WIN32
#include <windows.h> // @manual
// windows.h has to come first. Don't alphabetize, clang-format.
#include <processthreadsapi.h> // @manual
#else
#include <unistd.h> // @manual
#endif

namespace {

using namespace benchmark;
using namespace facebook::eden;

#ifdef _WIN32

void win32_GetCurrentProcess(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(GetCurrentProcessId());
  }
}
BENCHMARK(win32_GetCurrentProcess);

#else

void unix_getpid(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(getpid());
  }
}
BENCHMARK(unix_getpid);

#endif

void folly_get_cached_pid(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(folly::get_cached_pid());
  }
}
BENCHMARK(folly_get_cached_pid);

void ProcessId_current(benchmark::State& state) {
  for (auto _ : state) {
    ProcessId::current();
  }
}
BENCHMARK(ProcessId_current);

} // namespace
