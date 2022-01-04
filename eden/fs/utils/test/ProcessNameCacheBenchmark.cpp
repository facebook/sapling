/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ProcessNameCache.h"

#include <benchmark/benchmark.h>
#include "eden/fs/benchharness/Bench.h"

using namespace facebook::eden;

struct ProcessNameCacheFixture : benchmark::Fixture {
  ProcessNameCache processNameCache;
};

/**
 * A high but realistic amount of contention.
 */
constexpr size_t kThreadCount = 4;

BENCHMARK_DEFINE_F(ProcessNameCacheFixture, add_self)(benchmark::State& state) {
  auto myPid = getpid();
  for (auto _ : state) {
    processNameCache.add(myPid);
  }
}

BENCHMARK_REGISTER_F(ProcessNameCacheFixture, add_self)->Threads(kThreadCount);
