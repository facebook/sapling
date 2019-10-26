/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ScubaStructuredLogger.h"
#include <folly/json.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include "eden/fs/telemetry/ScribeLogger.h"

using namespace facebook::eden;
using namespace testing;

namespace {

struct TestScribeLogger : public ScribeLogger {
  std::vector<std::string> lines;

  void log(std::string line) override {
    lines.emplace_back(std::move(line));
  }
};

struct TestLogEvent {
  static constexpr const char* type = "test_event";

  std::string str;
  int number = 0;

  void populate(DynamicEvent& event) const {
    event.addString("str", str);
    event.addInt("number", number);
  }
};

struct ScubaStructuredLoggerTest : public ::testing::Test {
  std::shared_ptr<TestScribeLogger> scribe{
      std::make_shared<TestScribeLogger>()};
  ScubaStructuredLogger logger{scribe, SessionInfo{}};
};

} // namespace

TEST_F(ScubaStructuredLoggerTest, json_is_written_in_one_line) {
  logger.logEvent(TestLogEvent{"name", 10});
  EXPECT_EQ(1, scribe->lines.size());
  const auto& line = scribe->lines[0];
  auto index = line.find('\n');
  EXPECT_EQ(std::string::npos, index);
}

std::vector<std::string> keysOf(const folly::dynamic& d) {
  std::vector<std::string> rv;
  for (auto key : d.keys()) {
    rv.push_back(key.asString());
  }
  return rv;
}

TEST_F(ScubaStructuredLoggerTest, json_contains_types_at_top_level_and_values) {
  logger.logEvent(TestLogEvent{"name", 10});
  EXPECT_EQ(1, scribe->lines.size());
  const auto& line = scribe->lines[0];
  auto doc = folly::parseJson(line);
  EXPECT_TRUE(doc.isObject());
  EXPECT_THAT(keysOf(doc), UnorderedElementsAre("int", "normal"));

  auto ints = doc["int"];
  EXPECT_TRUE(ints.isObject());
  EXPECT_THAT(
      keysOf(ints), UnorderedElementsAre("time", "number", "session_id"));

  auto normals = doc["normal"];
  EXPECT_TRUE(normals.isObject());
  EXPECT_THAT(
      keysOf(normals),
      UnorderedElementsAre(
          "str", "user", "host", "type", "os", "osver", "edenver"));
}
