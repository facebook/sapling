/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Try.h>
#include "eden/common/utils/benchharness/Bench.h"

namespace {

using namespace benchmark;

void make_runtime_error(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(std::runtime_error("shortstr"));
  }
}
BENCHMARK(make_runtime_error);

void throw_and_catch(benchmark::State& state) {
  uint64_t count = 0;
  for (auto _ : state) {
    try {
      throw std::runtime_error("shortstr");
    } catch (const std::exception& e) {
      count += e.what()[0];
    }
  }
  benchmark::DoNotOptimize(count);
}
BENCHMARK(throw_and_catch);

void makeTryWith_thrown_exception(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(
        folly::makeTryWith([] { throw std::runtime_error("shortstr"); }));
  }
}
BENCHMARK(makeTryWith_thrown_exception);

void makeTryWith_constructed_exception(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(folly::Try<void>{std::runtime_error("shortstr")});
  }
}
BENCHMARK(makeTryWith_constructed_exception);

} // namespace
