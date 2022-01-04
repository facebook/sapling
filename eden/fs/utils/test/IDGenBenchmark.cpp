/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/IDGen.h"

#include <benchmark/benchmark.h>

using namespace facebook::eden;

static void BM_generateUniqueID(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(generateUniqueID());
  }
}
BENCHMARK(BM_generateUniqueID);
