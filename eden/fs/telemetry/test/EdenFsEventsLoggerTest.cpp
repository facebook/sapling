/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenFsEventsLogger.h"

#include <gtest/gtest.h>

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/common/telemetry/LogEvent.h"
#include "eden/common/telemetry/StructuredLogger.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/XplatKeys.h"
#include "eden/fs/telemetry/facebook/EdenTelemetryIdentity.h"
#include "eden/fs/telemetry/facebook/XplatLogger.h"

using namespace facebook::eden;

namespace {

class SpyStructuredLogger : public StructuredLogger {
 public:
  SpyStructuredLogger() : StructuredLogger(true, SessionInfo{}) {}
  std::atomic<int> callCount{0};

 protected:
  void logDynamicEvent(DynamicEvent) override {
    callCount.fetch_add(1);
  }
};

std::shared_ptr<ReloadableConfig> makeTestReloadableConfig(
    bool enableXplatLoggerEvents = false) {
  auto config = EdenConfig::createTestEdenConfig();
  config->enableXplatLoggerEvents.setValue(
      enableXplatLoggerEvents, ConfigSourceType::UserConfig, true);
  return std::make_shared<ReloadableConfig>(std::move(config));
}

class SpyXplatLogger : public XplatLogger {
 public:
  SpyXplatLogger()
      : XplatLogger(
            EdenTelemetryIdentity{},
            makeRefPtr<EdenStats>(),
            makeTestReloadableConfig()) {}

  std::atomic<int> callCount{0};
  std::string lastCategory;
  DynamicEvent lastEvent;

  void logEvent(std::string_view category, const DynamicEvent& event) override {
    lastCategory = std::string{category};
    lastEvent = event;
    callCount.fetch_add(1);
  }
};

struct TestTypedEvent : public TestEvent {
  std::string str;
  int number = 0;

  TestTypedEvent(std::string str, int number)
      : str(std::move(str)), number(number) {}

  void populate(DynamicEvent& event) const override {
    event.addString("str", str);
    event.addInt("number", number);
  }

  const char* getType() const override {
    return "test_typed_event";
  }
};

struct TestTypelessEvent : public TypelessTestEvent {
  std::string str;
  int number = 0;

  TestTypelessEvent(std::string str, int number)
      : str(std::move(str)), number(number) {}

  void populate(DynamicEvent& event) const override {
    event.addString("str", str);
    event.addInt("number", number);
  }
};

class EdenFsEventsLoggerTest : public ::testing::Test {
 protected:
  EdenFsEventsLogger createLogger(
      bool enableXplatLoggerEvents,
      std::shared_ptr<SpyStructuredLogger> spyLogger,
      XplatLogger* xplatLogger) {
    auto reloadableConfig = makeTestReloadableConfig(enableXplatLoggerEvents);
    return EdenFsEventsLogger(
        std::move(spyLogger),
        xplatLogger,
        std::move(reloadableConfig),
        makeRefPtr<EdenStats>());
  }
};

TEST_F(EdenFsEventsLoggerTest, typedEventRoutesToStructuredLoggerWhenDisabled) {
  auto spyLogger = std::make_shared<SpyStructuredLogger>();
  auto logger = createLogger(false, spyLogger, nullptr);

  logger.logEvent(TestTypedEvent{"hello", 42});

  EXPECT_EQ(1, spyLogger->callCount.load());
}

TEST_F(EdenFsEventsLoggerTest, typedEventRoutesToXplatLoggerWhenEnabled) {
  auto spyLogger = std::make_shared<SpyStructuredLogger>();
  SpyXplatLogger spyXplatLogger;
  auto logger = createLogger(true, spyLogger, &spyXplatLogger);

  logger.logEvent(TestTypedEvent{"hello", 42});

  EXPECT_EQ(1, spyXplatLogger.callCount.load());
  EXPECT_EQ(0, spyLogger->callCount.load());
  EXPECT_EQ(
      std::string{xplat_keys::kEventsCategory}, spyXplatLogger.lastCategory);

  const auto& strings = spyXplatLogger.lastEvent.getStringMap();
  EXPECT_EQ("hello", strings.at("str"));
  EXPECT_EQ("test_typed_event", strings.at(std::string{xplat_keys::kType}));

  const auto& ints = spyXplatLogger.lastEvent.getIntMap();
  EXPECT_EQ(42, ints.at("number"));
}

TEST_F(
    EdenFsEventsLoggerTest,
    typedEventRoutesToStructuredLoggerWhenXplatLoggerNull) {
  auto spyLogger = std::make_shared<SpyStructuredLogger>();
  auto logger = createLogger(true, spyLogger, nullptr);

  logger.logEvent(TestTypedEvent{"hello", 42});

  EXPECT_EQ(1, spyLogger->callCount.load());
}

TEST_F(
    EdenFsEventsLoggerTest,
    typelessEventRoutesToStructuredLoggerWhenDisabled) {
  auto spyLogger = std::make_shared<SpyStructuredLogger>();
  auto logger = createLogger(false, spyLogger, nullptr);

  logger.logEvent(TestTypelessEvent{"world", 99});

  EXPECT_EQ(1, spyLogger->callCount.load());
}

TEST_F(EdenFsEventsLoggerTest, typelessEventRoutesToXplatLoggerWhenEnabled) {
  auto spyLogger = std::make_shared<SpyStructuredLogger>();
  SpyXplatLogger spyXplatLogger;
  auto logger = createLogger(true, spyLogger, &spyXplatLogger);

  logger.logEvent(TestTypelessEvent{"world", 99});

  EXPECT_EQ(1, spyXplatLogger.callCount.load());
  EXPECT_EQ(0, spyLogger->callCount.load());
  EXPECT_EQ(
      std::string{xplat_keys::kEventsCategory}, spyXplatLogger.lastCategory);

  const auto& strings = spyXplatLogger.lastEvent.getStringMap();
  EXPECT_EQ("world", strings.at("str"));
  // TypelessEvent should NOT have the type field
  EXPECT_EQ(strings.end(), strings.find(std::string{xplat_keys::kType}));

  const auto& ints = spyXplatLogger.lastEvent.getIntMap();
  EXPECT_EQ(99, ints.at("number"));
}

} // namespace
