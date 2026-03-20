/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Benchmark.h>
#include <folly/init/Init.h>

#include "eden/fs/inodes/InodePressurePolicy.h"

using namespace facebook::eden;

namespace {

// Parameters that ensure interpolate() hits the full power-curve math
// (not the early-return paths).
constexpr uint64_t kMinInodeCount = 100'000;
constexpr uint64_t kMaxInodeCount = 10'000'000;
// Midpoint in log-space to exercise the full interpolation path
constexpr uint64_t kMidInodeCount = 1'000'000;

InodePressurePolicy makePolicy() {
  return InodePressurePolicy(
      kMinInodeCount,
      kMaxInodeCount,
      std::chrono::seconds{3600},
      std::chrono::seconds{60},
      std::chrono::seconds{7200},
      std::chrono::seconds{300});
}

} // namespace

BENCHMARK(getFuseTtl, iters) {
  auto policy = makePolicy();
  for (unsigned i = 0; i < iters; ++i) {
    auto ttl = policy.getFuseTtl(kMidInodeCount);
    folly::doNotOptimizeAway(ttl);
  }
}

BENCHMARK(getFuseTtl_varyingInodes, iters) {
  auto policy = makePolicy();
  for (unsigned i = 0; i < iters; ++i) {
    auto ttl = policy.getFuseTtl(kMidInodeCount + i);
    folly::doNotOptimizeAway(ttl);
  }
}

int main(int argc, char** argv) {
  folly::Init init(&argc, &argv);
  folly::runBenchmarks();
  return 0;
}
