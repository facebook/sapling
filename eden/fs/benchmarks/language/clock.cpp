/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <chrono>
#include "eden/common/utils/benchharness/Bench.h"

namespace {

using namespace benchmark;
using namespace facebook::eden;

void system_clock(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(std::chrono::system_clock::now());
  }
}
BENCHMARK(system_clock);

void steady_clock(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(std::chrono::steady_clock::now());
  }
}
BENCHMARK(steady_clock);

void high_resolution_clock(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(std::chrono::high_resolution_clock::now());
  }
}
BENCHMARK(high_resolution_clock);

} // namespace
