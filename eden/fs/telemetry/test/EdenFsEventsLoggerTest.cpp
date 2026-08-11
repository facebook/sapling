/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenFsEventsLogger.h"

#include <gtest/gtest.h>
#include <memory>
#include <string>
#include <utility>

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/common/telemetry/LogEvent.h"
#include "eden/fs/telemetry/IXplatLogger.h"
#include "eden/fs/telemetry/XplatKeys.h"

using namespace facebook::eden;

namespace {

class SpyXplatLogger : public IXplatLogger {
 public:
  int callCount{0};
  std::string lastCategory;
  DynamicEvent lastEvent;

  void logEvent(std::string_view category, const DynamicEvent& event) override {
    lastCategory = std::string{category};
    lastEvent = event;
    ++callCount;
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

TEST(EdenFsEventsLoggerTest, typedEventUsesRetainedXplatLogger) {
  auto spyXplatLogger = std::make_shared<SpyXplatLogger>();
  std::weak_ptr<SpyXplatLogger> weakXplatLogger = spyXplatLogger;
  EdenFsEventsLogger logger{spyXplatLogger};
  spyXplatLogger.reset();

  logger.logEvent(TestTypedEvent{"hello", 42});

  auto retainedXplatLogger = weakXplatLogger.lock();
  ASSERT_NE(nullptr, retainedXplatLogger);
  EXPECT_EQ(1, retainedXplatLogger->callCount);
  EXPECT_EQ(
      std::string{xplat_keys::kEventsCategory},
      retainedXplatLogger->lastCategory);

  const auto& strings = retainedXplatLogger->lastEvent.getStringMap();
  EXPECT_EQ("hello", strings.at("str"));
  EXPECT_EQ("test_typed_event", strings.at(std::string{xplat_keys::kType}));

  const auto& ints = retainedXplatLogger->lastEvent.getIntMap();
  EXPECT_EQ(42, ints.at("number"));
}

TEST(EdenFsEventsLoggerTest, typelessEventOmitsType) {
  auto spyXplatLogger = std::make_shared<SpyXplatLogger>();
  EdenFsEventsLogger logger{spyXplatLogger};

  logger.logEvent(TestTypelessEvent{"world", 99});

  EXPECT_EQ(1, spyXplatLogger->callCount);
  EXPECT_EQ(
      std::string{xplat_keys::kEventsCategory}, spyXplatLogger->lastCategory);

  const auto& strings = spyXplatLogger->lastEvent.getStringMap();
  EXPECT_EQ("world", strings.at("str"));
  EXPECT_EQ(strings.end(), strings.find(std::string{xplat_keys::kType}));

  const auto& ints = spyXplatLogger->lastEvent.getIntMap();
  EXPECT_EQ(99, ints.at("number"));
}

} // namespace
