/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Benchmark.h>
#include <folly/init/Init.h>
#include <folly/synchronization/test/Barrier.h>
#include "eden/fs/benchharness/Bench.h"
#include "eden/fs/tracing/Tracing.h"

using namespace facebook::eden;

BENCHMARK(Tracer_repeatedly_create_trace_points, n) {
  {
    folly::BenchmarkSuspender suspender;
    enableTracing();
  }
  for (unsigned i = 0; i < n; ++i) {
    TraceBlock block{"foo"};
  }
}

BENCHMARK(Tracer_repeatedly_create_trace_points_from_multiple_threads, n) {
  constexpr unsigned threadCount = 8;
  std::vector<std::thread> threads;
  folly::test::Barrier gate{1 + threadCount};
  {
    folly::BenchmarkSuspender suspender;
    enableTracing();

    for (unsigned i = 0; i < threadCount; ++i) {
      threads.emplace_back([n, &gate] {
        gate.wait();
        // We aren't measuring the time of these other threads, so
        // double the number of trace points to keep things busy
        // while the main thread creates the requested number of
        // tracepoints
        for (unsigned i = 0; i < n * 2; ++i) {
          TraceBlock block{"foo"};
        }
      });
    }
    gate.wait();
  }
  for (unsigned i = 0; i < n; ++i) {
    TraceBlock block{"foo"};
  }
  folly::BenchmarkSuspender suspender;
  for (auto& thread : threads) {
    thread.join();
  }
}

BENCHMARK(Tracer_repeatedly_create_trace_points_disabled, n) {
  {
    folly::BenchmarkSuspender suspender;
    disableTracing();
  }
  for (unsigned i = 0; i < n; ++i) {
    TraceBlock block{"foo"};
  }
}

int main(int argc, char** argv) {
  folly::init(&argc, &argv);
  folly::runBenchmarks();
}
