/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace {

using namespace facebook::eden;

void immediate_future(benchmark::State& state) {
  ImmediateFuture<uint64_t> fut{};

  for (auto _ : state) {
    auto newFut = std::move(fut).thenValue([](uint64_t v) { return v + 1; });
    fut = std::move(newFut);
  }
  state.SetItemsProcessed(std::move(fut).get());
}

void immediate_future_exc(benchmark::State& state) {
  ImmediateFuture<uint64_t> fut{folly::Try<uint64_t>{std::logic_error("Foo")}};

  uint64_t processed = 0;
  for (auto _ : state) {
    auto newFut = std::move(fut).thenValue([](uint64_t v) { return v + 1; });
    fut = std::move(newFut);
    processed++;
  }
  benchmark::DoNotOptimize(fut);
  state.SetItemsProcessed(processed);
}

void folly_future(benchmark::State& state) {
  folly::Future<int> fut{0};
  for (auto _ : state) {
    auto newFut = std::move(fut).thenValue([](int v) { return v + 1; });
    fut = std::move(newFut);
  }
  state.SetItemsProcessed(std::move(fut).get());
}

BENCHMARK(immediate_future);
BENCHMARK(immediate_future_exc);
BENCHMARK(folly_future);
} // namespace
