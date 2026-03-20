/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodePressurePolicy.h"

#include <gtest/gtest.h>

using namespace facebook::eden;
using namespace std::chrono_literals;

namespace {

// Standard test policy: pressure ramps between 100k and 1M inodes.
InodePressurePolicy makeStandardPolicy() {
  return InodePressurePolicy(
      /*minInodeCount=*/100'000,
      /*maxInodeCount=*/1'000'000,
      /*fuseTtlMax=*/600s,
      /*fuseTtlMin=*/10s,
      /*gcCutoffMax=*/3600s,
      /*gcCutoffMin=*/60s);
}

} // namespace

TEST(InodePressurePolicy, BelowMinReturnsMaxValues) {
  auto policy = makeStandardPolicy();

  EXPECT_EQ(600s, policy.getFuseTtl(0));
  EXPECT_EQ(600s, policy.getFuseTtl(100'000));
  EXPECT_EQ(3600s, policy.getGcCutoff(0));
  EXPECT_EQ(3600s, policy.getGcCutoff(100'000));
}

TEST(InodePressurePolicy, AboveMaxReturnsMinValues) {
  auto policy = makeStandardPolicy();

  EXPECT_EQ(10s, policy.getFuseTtl(1'000'000));
  EXPECT_EQ(10s, policy.getFuseTtl(10'000'000));
  EXPECT_EQ(60s, policy.getGcCutoff(1'000'000));
  EXPECT_EQ(60s, policy.getGcCutoff(10'000'000));
}

TEST(InodePressurePolicy, ContinuousInterpolationInRange) {
  auto policy = makeStandardPolicy();

  // Just above min: should be close to max (quadratic ease-in)
  auto ttlLow = policy.getFuseTtl(110'000);
  EXPECT_LT(ttlLow, 600s);
  EXPECT_GT(ttlLow, 500s); // k=2 keeps values near max at low pressure

  auto gcLow = policy.getGcCutoff(110'000);
  EXPECT_LT(gcLow, 3600s);
  EXPECT_GT(gcLow, 3000s);

  // Near max: should be close to min
  auto ttlHigh = policy.getFuseTtl(900'000);
  EXPECT_LT(ttlHigh, 50s);
  EXPECT_GT(ttlHigh, 10s);

  auto gcHigh = policy.getGcCutoff(900'000);
  EXPECT_LT(gcHigh, 300s);
  EXPECT_GT(gcHigh, 60s);
}

TEST(InodePressurePolicy, ValuesDecreaseMonotonically) {
  auto policy = makeStandardPolicy();

  std::chrono::seconds prevTtl = 601s;
  std::chrono::seconds prevGc = 3601s;
  for (uint64_t count = 100'000; count <= 1'000'000; count += 50'000) {
    auto ttl = policy.getFuseTtl(count);
    auto gc = policy.getGcCutoff(count);
    EXPECT_LE(ttl, prevTtl) << "at count " << count;
    EXPECT_LE(gc, prevGc) << "at count " << count;
    prevTtl = ttl;
    prevGc = gc;
  }
}

TEST(InodePressurePolicy, QuadraticEaseInStaysHighAtLowPressure) {
  // Verify the k=2 curve property: at 25% of the inode range (in log-space),
  // the value should still be very close to max. The quadratic exponent
  // means only ~6% of the value range has been consumed at t=0.25.
  auto policy = makeStandardPolicy();

  // 25% through log-space: 100k * (1M/100k)^0.25 ≈ 178k
  // At t=0.25, t^2=0.0625, so only ~6% of the exponential ramp is consumed.
  auto ttlQuarter = policy.getFuseTtl(178'000);
  EXPECT_GT(ttlQuarter, 400s)
      << "quadratic ease-in should stay near max at low pressure";

  // Geometric midpoint (~316k) should still be well above min
  auto ttlMid = policy.getFuseTtl(316'000);
  EXPECT_GT(ttlMid, 100s) << "midpoint should be far from min value";
}

TEST(InodePressurePolicy, EqualMinMaxProducesConstantValues) {
  InodePressurePolicy policy(
      /*minInodeCount=*/100'000,
      /*maxInodeCount=*/1'000'000,
      /*fuseTtlMax=*/300s,
      /*fuseTtlMin=*/300s,
      /*gcCutoffMax=*/1800s,
      /*gcCutoffMin=*/1800s);

  EXPECT_EQ(300s, policy.getFuseTtl(0));
  EXPECT_EQ(300s, policy.getFuseTtl(500'000));
  EXPECT_EQ(300s, policy.getFuseTtl(2'000'000));
  EXPECT_EQ(1800s, policy.getGcCutoff(0));
  EXPECT_EQ(1800s, policy.getGcCutoff(500'000));
  EXPECT_EQ(1800s, policy.getGcCutoff(2'000'000));
}

TEST(InodePressurePolicy, EqualInodeCountsUsesMaxValues) {
  InodePressurePolicy policy(
      /*minInodeCount=*/500'000,
      /*maxInodeCount=*/500'000,
      /*fuseTtlMax=*/600s,
      /*fuseTtlMin=*/10s,
      /*gcCutoffMax=*/3600s,
      /*gcCutoffMin=*/60s);

  // At or below → max, above → min
  EXPECT_EQ(600s, policy.getFuseTtl(500'000));
  EXPECT_EQ(10s, policy.getFuseTtl(500'001));
  EXPECT_EQ(3600s, policy.getGcCutoff(500'000));
  EXPECT_EQ(60s, policy.getGcCutoff(500'001));
}
