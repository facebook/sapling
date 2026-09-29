/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EventRateLimiter.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

namespace {

using Result = std::optional<uint64_t>;

TEST(EventRateLimiterTest, AdmitsBurstThenSuppressesAndReportsCount) {
  double now = 1000.0;
  EventRateLimiter limiter{[&] { return now; }};

  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 2.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 2.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 2.0), std::nullopt);
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 2.0), std::nullopt);
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 2.0), std::nullopt);

  now += 1.0;
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 2.0), Result{3});
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 2.0), std::nullopt);

  now += 1.0;
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 2.0), Result{1});
}

TEST(EventRateLimiterTest, KeysAreIndependent) {
  double now = 1000.0;
  EventRateLimiter limiter{[&] { return now; }};

  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 1.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 1.0), std::nullopt);
  EXPECT_EQ(limiter.tryAcquire("b", 1.0, 1.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("b", 1.0, 1.0), std::nullopt);
}

TEST(EventRateLimiterTest, NonPositiveRateAdmitsEverything) {
  double now = 1000.0;
  EventRateLimiter limiter{[&] { return now; }};

  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 1.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 1.0), std::nullopt);
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 1.0), std::nullopt);

  EXPECT_EQ(limiter.tryAcquire("a", 0.0, 1.0), Result{2});
  EXPECT_EQ(limiter.tryAcquire("a", 0.0, 1.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", -5.0, 0.0), Result{0});
}

TEST(EventRateLimiterTest, FullBurstAvailableWhenClockStartsNearZero) {
  // folly::DynamicTokenBucket counts tokens from its zero time, so a clock
  // that has barely advanced (steady_clock right after boot) must not shrink
  // the initial burst.
  double now = 0.5;
  EventRateLimiter limiter{[&] { return now; }};

  EXPECT_EQ(limiter.tryAcquire("a", 0.1, 3.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", 0.1, 3.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", 0.1, 3.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", 0.1, 3.0), std::nullopt);
}

TEST(EventRateLimiterTest, BurstIsClampedToOne) {
  double now = 1000.0;
  EventRateLimiter limiter{[&] { return now; }};

  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 0.0), Result{0});
  EXPECT_EQ(limiter.tryAcquire("a", 1.0, 0.0), std::nullopt);
}

} // namespace
