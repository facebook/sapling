/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/mononoke/adaptive_rate_limiter/cpp/AdaptiveRateLimiterWrapper.h"

#include <gtest/gtest.h>

namespace facebook::mononoke {

class AdaptiveRateLimiterTest : public ::testing::Test {
 protected:
  void SetUp() override {}
  void TearDown() override {}
};

TEST_F(AdaptiveRateLimiterTest, CreateWithDefaultConfig) {
  // Test that we can create a rate limiter with default config
  // (which has thresholds at 1.0, so no shedding should occur)
  AdaptiveRateLimiterConfig config;
  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // With default config (all thresholds at 1.0), should never shed
  // unless CPU/memory is at 100%
  // Just verify the call doesn't crash - actual result depends on system load
  (void)limiter->shouldShed();
  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, CreateWithCustomConfig) {
  // Test creating with realistic thresholds
  AdaptiveRateLimiterConfig config(
      0.7, // CPU soft limit: 70%
      0.85, // CPU hard limit: 85%
      0.8, // Memory soft limit: 80%
      0.95, // Memory hard limit: 95%
      100 // Update period: 100ms
  );

  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // Should be able to call shouldShed without crashing
  limiter->shouldShed();
  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, UpdateConfig) {
  // Test that we can update configuration at runtime
  AdaptiveRateLimiterConfig initialConfig;
  auto limiter = AdaptiveRateLimiterWrapper::create(initialConfig);

  // Update with stricter thresholds
  AdaptiveRateLimiterConfig strictConfig(
      0.5, // CPU soft: 50%
      0.7, // CPU hard: 70%
      0.6, // Memory soft: 60%
      0.8 // Memory hard: 80%
  );

  limiter->updateConfig(strictConfig);

  // Verify we can still call shouldShed
  limiter->shouldShed();
  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, MultipleShedChecks) {
  // Test that we can call shouldShed multiple times
  AdaptiveRateLimiterConfig config(0.7, 0.85, 0.8, 0.95);
  auto limiter = AdaptiveRateLimiterWrapper::create(config);

  // Call multiple times
  for (int i = 0; i < 100; ++i) {
    limiter->shouldShed();
  }

  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, ConfigValidation) {
  // Test various config values
  AdaptiveRateLimiterConfig config1(0.0, 0.0, 0.0, 0.0); // Very loose
  auto limiter1 = AdaptiveRateLimiterWrapper::create(config1);
  ASSERT_NE(limiter1, nullptr);

  AdaptiveRateLimiterConfig config2(0.5, 0.5, 0.5, 0.5); // Same soft/hard
  auto limiter2 = AdaptiveRateLimiterWrapper::create(config2);
  ASSERT_NE(limiter2, nullptr);

  AdaptiveRateLimiterConfig config3(1.0, 1.0, 1.0, 1.0); // Very strict
  auto limiter3 = AdaptiveRateLimiterWrapper::create(config3);
  ASSERT_NE(limiter3, nullptr);
}

TEST_F(AdaptiveRateLimiterTest, MonitoringModeNone) {
  // Test that NONE mode disables rate limiting
  AdaptiveRateLimiterConfig config(
      ResourceMonitoringMode::NONE,
      0.0, // Very strict thresholds
      0.0,
      0.0,
      0.0);

  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // Should never shed when monitoring is disabled
  for (int i = 0; i < 10; ++i) {
    EXPECT_FALSE(limiter->shouldShed());
  }
}

TEST_F(AdaptiveRateLimiterTest, MonitoringModeCgroupOnly) {
  // Test cgroup-only monitoring
  AdaptiveRateLimiterConfig config(
      ResourceMonitoringMode::CGROUP_ONLY, 0.7, 0.85, 0.8, 0.95);

  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // Should be able to call shouldShed
  (void)limiter->shouldShed();
  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, MonitoringModeHostOnly) {
  // Test host-only monitoring
  AdaptiveRateLimiterConfig config(
      ResourceMonitoringMode::HOST_ONLY, 0.7, 0.85, 0.8, 0.95);

  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // Should be able to call shouldShed
  (void)limiter->shouldShed();
  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, MonitoringModeBoth) {
  // Test multi-level monitoring (both host and cgroup)
  AdaptiveRateLimiterConfig config(
      ResourceMonitoringMode::BOTH, 0.7, 0.85, 0.8, 0.95);

  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // Should be able to call shouldShed with multi-level monitoring
  (void)limiter->shouldShed();
  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, UpdateConfigWithModeChange) {
  // Test that we can change monitoring mode at runtime
  AdaptiveRateLimiterConfig initialConfig(
      ResourceMonitoringMode::CGROUP_ONLY, 0.7, 0.85, 0.8, 0.95);

  auto limiter = AdaptiveRateLimiterWrapper::create(initialConfig);

  // Change to HOST_ONLY mode
  AdaptiveRateLimiterConfig newConfig(
      ResourceMonitoringMode::HOST_ONLY, 0.6, 0.8, 0.7, 0.9);

  limiter->updateConfig(newConfig);

  // Should still work after mode change
  (void)limiter->shouldShed();
  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, OperationModeDisabled) {
  // Test that DISABLED mode never sheds
  AdaptiveRateLimiterConfig config(
      OperationMode::DISABLED,
      ResourceMonitoringMode::CGROUP_ONLY,
      0.0, // Very strict thresholds - should shed if enabled
      0.0,
      0.0,
      0.0);

  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // Should never shed when operation mode is DISABLED
  for (int i = 0; i < 10; ++i) {
    EXPECT_FALSE(limiter->shouldShed());
  }
}

TEST_F(AdaptiveRateLimiterTest, OperationModeDryRun) {
  // Test that DRY_RUN mode never actually sheds
  AdaptiveRateLimiterConfig config(
      OperationMode::DRY_RUN,
      ResourceMonitoringMode::CGROUP_ONLY,
      0.0, // Very strict thresholds - would shed if enabled
      0.0,
      0.0,
      0.0);

  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // Should never shed in DRY_RUN mode (logs but doesn't shed)
  for (int i = 0; i < 10; ++i) {
    EXPECT_FALSE(limiter->shouldShed());
  }
}

TEST_F(AdaptiveRateLimiterTest, OperationModeEnabled) {
  // Test that ENABLED mode can shed
  AdaptiveRateLimiterConfig config(
      OperationMode::ENABLED,
      ResourceMonitoringMode::CGROUP_ONLY,
      0.7,
      0.85,
      0.8,
      0.95);

  auto limiter = AdaptiveRateLimiterWrapper::create(config);
  ASSERT_NE(limiter, nullptr);

  // Should be able to call shouldShed
  // Actual shedding depends on system load
  (void)limiter->shouldShed();
  SUCCEED();
}

TEST_F(AdaptiveRateLimiterTest, UpdateOperationMode) {
  // Test changing operation mode at runtime
  AdaptiveRateLimiterConfig initialConfig(
      OperationMode::DISABLED,
      ResourceMonitoringMode::CGROUP_ONLY,
      0.0,
      0.0,
      0.0,
      0.0);

  auto limiter = AdaptiveRateLimiterWrapper::create(initialConfig);

  // Initially DISABLED - should never shed
  EXPECT_FALSE(limiter->shouldShed());

  // Change to DRY_RUN mode
  AdaptiveRateLimiterConfig dryRunConfig(
      OperationMode::DRY_RUN,
      ResourceMonitoringMode::CGROUP_ONLY,
      0.0,
      0.0,
      0.0,
      0.0);

  limiter->updateConfig(dryRunConfig);

  // DRY_RUN should also not shed
  EXPECT_FALSE(limiter->shouldShed());

  // Change to ENABLED mode
  AdaptiveRateLimiterConfig enabledConfig(
      OperationMode::ENABLED,
      ResourceMonitoringMode::CGROUP_ONLY,
      0.7,
      0.85,
      0.8,
      0.95);

  limiter->updateConfig(enabledConfig);

  // ENABLED mode - can shed based on system load
  (void)limiter->shouldShed();
  SUCCEED();
}

} // namespace facebook::mononoke
