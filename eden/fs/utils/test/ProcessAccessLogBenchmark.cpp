/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/ProcessAccessLog.h"

#include <folly/Benchmark.h>
#include "eden/fs/benchharness/Bench.h"
#include "eden/fs/utils/ProcessNameCache.h"

using namespace facebook::eden;

/**
 * A high but realistic amount of contention.
 */
constexpr size_t kThreadCount = 4;

BENCHMARK(ProcessAccessLog_repeatedly_add_self, iters) {
  folly::BenchmarkSuspender suspender;

  auto processNameCache = std::make_shared<ProcessNameCache>();
  ProcessAccessLog processAccessLog{processNameCache};

  std::vector<std::thread> threads;
  StartingGate gate{kThreadCount};

  size_t remainingIterations = iters;
  size_t totalIterations = 0;
  for (size_t i = 0; i < kThreadCount; ++i) {
    size_t remainingThreads = kThreadCount - i;
    size_t assignedIterations = remainingIterations / remainingThreads;
    remainingIterations -= assignedIterations;
    totalIterations += assignedIterations;
    threads.emplace_back(
        [&processAccessLog, &gate, assignedIterations, myPid = getpid()] {
          gate.wait();
          for (size_t j = 0; j < assignedIterations; ++j) {
            processAccessLog.recordAccess(
                myPid, ProcessAccessLog::AccessType::FuseOther);
          }
        });
  }

  CHECK_EQ(totalIterations, iters);

  suspender.dismiss();

  // Now wake the threads.
  gate.waitThenOpen();

  // Wait until they're done.
  for (auto& thread : threads) {
    thread.join();
  }
}
