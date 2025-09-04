/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/common/os/ProcessId.h"
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/store/ObjectStore.h"

using namespace facebook::eden;

PidFetchCounts counts{};

void pid_fetch_count(benchmark::State& state) {
  if (state.thread_index() == 0) {
    counts.clear();
  }

  auto pid = ProcessId::current();
  for (auto _ : state) {
    counts.recordProcessFetch(pid);
  }
}

BENCHMARK(pid_fetch_count)->ThreadRange(1, 128);

EDEN_BENCHMARK_MAIN();
