/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/FsEventLogger.h"

#include <gtest/gtest.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/IScribeLogger.h"

using namespace facebook::eden;

namespace {

struct CountingScribeLogger : NullScribeLogger {
  size_t samples = 0;

  void logFsEventSample(FsEventSample) override {
    ++samples;
  }
};

struct LoggerWithCounter {
  std::shared_ptr<CountingScribeLogger> scribe;
  std::shared_ptr<FsEventLogger> logger;
};

LoggerWithCounter makeLogger(
    std::vector<uint32_t> denominators,
    uint32_t samplesPerMinute) {
  auto config = EdenConfig::createTestEdenConfig();
  config->requestSamplingGroupDenominators.setValue(
      std::move(denominators), ConfigSourceType::UserConfig);
  config->requestSamplesPerMinute.setValue(
      samplesPerMinute, ConfigSourceType::UserConfig);
  auto scribe = std::make_shared<CountingScribeLogger>();
  auto logger = std::make_shared<FsEventLogger>(
      std::make_shared<ReloadableConfig>(std::move(config)), scribe);
  return {scribe, logger};
}

FsEventLogger::Event eventInGroup(SamplingGroup group) {
  return {std::chrono::nanoseconds{1000}, group, "FUSE_LOOKUP"};
}

} // namespace

TEST(FsEventLoggerTest, samplingGroupEqualToDenominatorCount) {
  auto [scribe, logger] = makeLogger({0, 10, 100, 1000}, 10);
  EXPECT_NO_THROW(logger->log(eventInGroup(SamplingGroup::Four)));
  EXPECT_EQ(0, scribe->samples);
}

TEST(FsEventLoggerTest, samplingGroupPastDenominatorCount) {
  auto [scribe, logger] = makeLogger({0, 10, 100, 1000}, 10);
  EXPECT_NO_THROW(logger->log(eventInGroup(SamplingGroup::Five)));
  EXPECT_EQ(0, scribe->samples);
}
