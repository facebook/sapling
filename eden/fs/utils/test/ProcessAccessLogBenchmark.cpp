/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ProcessAccessLog.h"

#include <benchmark/benchmark.h>
#include "eden/fs/benchharness/Bench.h"
#include "eden/fs/utils/ProcessNameCache.h"

using namespace facebook::eden;

struct ProcessAccessLogFixture : benchmark::Fixture {
  std::shared_ptr<ProcessNameCache> processNameCache{
      std::make_shared<ProcessNameCache>()};
  ProcessAccessLog processAccessLog{processNameCache};
};

/**
 * A high but realistic amount of contention.
 */
constexpr size_t kThreadCount = 4;

BENCHMARK_DEFINE_F(ProcessAccessLogFixture, add_self)(benchmark::State& state) {
  auto myPid = getpid();
  for (auto _ : state) {
    processAccessLog.recordAccess(
        myPid, ProcessAccessLog::AccessType::FsChannelOther);
  }
}

BENCHMARK_REGISTER_F(ProcessAccessLogFixture, add_self)->Threads(kThreadCount);
