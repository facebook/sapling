/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/RefPtr.h"

#include <benchmark/benchmark.h>

#include <memory>

namespace {

using namespace facebook::eden;

struct Empty {};

struct Ref final : RefCounted {};

void make_unique_ptr(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(std::make_unique<Empty>());
  }
}
BENCHMARK(make_unique_ptr);

void make_shared_ptr(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(std::make_shared<Empty>());
  }
}
BENCHMARK(make_shared_ptr);

void make_ref_ptr(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(makeRefPtr<Ref>());
  }
}
BENCHMARK(make_ref_ptr);

void copy_shared_ptr(benchmark::State& state) {
  auto ptr = std::make_shared<Empty>();
  for (auto _ : state) {
    std::shared_ptr<Empty>{ptr};
  }
}
BENCHMARK(copy_shared_ptr);

void copy_ref_ptr(benchmark::State& state) {
  auto ptr = makeRefPtr<Ref>();
  for (auto _ : state) {
    ptr.copy();
  }
}
BENCHMARK(copy_ref_ptr);

} // namespace
