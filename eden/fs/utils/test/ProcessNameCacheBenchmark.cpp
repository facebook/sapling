/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/ProcessNameCache.h"

#include <folly/Benchmark.h>
#include "eden/fs/benchharness/Bench.h"

using namespace facebook::eden;

/**
 * A high but realistic amount of contention.
 */
constexpr size_t kThreadCount = 4;

BENCHMARK(ProcessNameCache_repeatedly_add_self, iters) {
  folly::BenchmarkSuspender suspender;

  ProcessNameCache processNameCache;
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
        [&processNameCache, &gate, assignedIterations, myPid = getpid()] {
          gate.wait();
          for (size_t j = 0; j < assignedIterations; ++j) {
            processNameCache.add(myPid);
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
