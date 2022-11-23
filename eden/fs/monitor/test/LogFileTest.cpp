/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <chrono>

#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>

#include "eden/fs/monitor/LogFile.h"
#include "eden/fs/monitor/LogRotation.h"
#include "eden/fs/testharness/FakeClock.h"
#include "eden/fs/testharness/TempFile.h"

namespace fs = boost::filesystem;
using namespace std::chrono_literals;
using folly::StringPiece;
using std::make_shared;
using std::make_unique;
using std::string;
using ::testing::UnorderedElementsAre;

namespace {
std::vector<std::string> listDir(const fs::path& path) {
  std::vector<std::string> results;
  for (const auto& entry : fs::directory_iterator(path)) {
    results.push_back(entry.path().filename().string());
  }
  return results;
}
} // namespace

namespace facebook::eden {

TEST(TimestampLogRotation, rotation) {
  auto tempdir = makeTempDir();
  auto dir = canonicalPath(tempdir.path().native());
  auto logPath = dir + "test.log"_pc;
  XLOG(DBG1) << "log path: " << logPath;

  // Set a very small file size limit, so that we exceed it with each message
  constexpr size_t maxFileSize = 10;
  constexpr size_t filesToKeep = 5;

  // Use a FakeClock object, starting at 2020-03-07 12:34:56
  auto clock = make_shared<FakeClock>();
  struct tm testTime = {};
  testTime.tm_year = 2020 - 1900;
  testTime.tm_mon = 3 - 1;
  testTime.tm_mday = 7;
  testTime.tm_hour = 12;
  testTime.tm_min = 34;
  testTime.tm_sec = 56;
  clock->set(FakeClock::time_point(std::chrono::seconds(mktime(&testTime))));

  {
    LogFile log(
        logPath,
        maxFileSize,
        make_unique<TimestampLogRotation>(filesToKeep, clock));
    string data(60, 'a');
    for (size_t n = 0; n < 100; ++n) {
      auto msg = folly::to<string>("msg ", n, ": ", data, "\n");
      log.write(msg.data(), msg.size());
      clock->advance(300ms);
    }
  }

  // At the end we should have the main log file, plus the most recent 5 rotated
  // files.  We updated the clock 99 times before the last rotation, 300ms each,
  // so the last rotation should be at 12:35:25.700
  auto files = listDir(tempdir.path());
  EXPECT_EQ(files.size(), filesToKeep + 1);
  EXPECT_THAT(
      listDir(tempdir.path()),
      UnorderedElementsAre(
          "test.log",
          "test.log-20200307.123524.1",
          "test.log-20200307.123524.2",
          "test.log-20200307.123525",
          "test.log-20200307.123525.1",
          "test.log-20200307.123525.2"));
}

TEST(TimestampLogRotation, removeOldLogFiles) {
  auto tempdir = makeTempDir();
  auto dir = canonicalPath(tempdir.path().native());
  auto logPath = dir + "test.log"_pc;
  XLOG(DBG1) << "log path: " << logPath;

  auto createFile = [&](StringPiece name) {
    auto full_path = dir + PathComponent(name);
    folly::File f(full_path.c_str(), O_CREAT | O_WRONLY | O_CLOEXEC, 0644);
  };

  createFile("test.log");
  createFile("test.log-20191231.235959");
  createFile("test.log-20200302.134258");
  createFile("test.log-20200303.001122");
  createFile("test.log-20200303.001122.1");
  createFile("test.log-20200303.001122.2");
  createFile("test.log-20200303.131122");
  createFile("test.log-20200305.235959");

  auto rotater = make_unique<TimestampLogRotation>(/*numFilesToKeep=*/5);
  // init() will perform an initial clean-up of old log files.
  rotater->init(logPath);
  EXPECT_THAT(
      listDir(tempdir.path()),
      UnorderedElementsAre(
          "test.log",
          "test.log-20200303.001122",
          "test.log-20200303.001122.1",
          "test.log-20200303.001122.2",
          "test.log-20200303.131122",
          "test.log-20200305.235959"));

  createFile("test.log-20200306.010203");
  rotater->removeOldLogFiles();
  EXPECT_THAT(
      listDir(tempdir.path()),
      UnorderedElementsAre(
          "test.log",
          "test.log-20200303.001122.1",
          "test.log-20200303.001122.2",
          "test.log-20200303.131122",
          "test.log-20200305.235959",
          "test.log-20200306.010203"));

  createFile("test.log-20200306.101234");
  rotater->removeOldLogFiles();
  EXPECT_THAT(
      listDir(tempdir.path()),
      UnorderedElementsAre(
          "test.log",
          "test.log-20200303.001122.2",
          "test.log-20200303.131122",
          "test.log-20200305.235959",
          "test.log-20200306.010203",
          "test.log-20200306.101234"));

  // Replace the rotation strategy with one that only keeps 2 old files
  rotater = make_unique<TimestampLogRotation>(/*numFilesToKeep=*/2);
  rotater->init(logPath);
  EXPECT_THAT(
      listDir(tempdir.path()),
      UnorderedElementsAre(
          "test.log", "test.log-20200306.010203", "test.log-20200306.101234"));
}

TEST(TimestampLogRotation, parseLogSuffix) {
  using FileSuffix = TimestampLogRotation::FileSuffix;
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20200302.123456"),
      FileSuffix(20200302, 123456, 0));
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("00000001.123456.1"),
      FileSuffix(1, 123456, 1));
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20201231.123456.078"),
      FileSuffix(20201231, 123456, 78));

  EXPECT_EQ(TimestampLogRotation::parseLogSuffix(".txt"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20201231.123456."), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20201231.123456_1"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20200302_123456"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20201231_123456_1"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("2020030.123456"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20200301.12345"), std::nullopt);
  EXPECT_EQ(TimestampLogRotation::parseLogSuffix("1.2"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20200302.-23456"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20200302.123456.-1"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20200302.123456.ff"), std::nullopt);
  EXPECT_EQ(
      TimestampLogRotation::parseLogSuffix("20200302.123456.0xff"),
      std::nullopt);
}

TEST(TimestampLogRotation, appendLogSuffix) {
  using FileSuffix = TimestampLogRotation::FileSuffix;
  EXPECT_EQ(
      TimestampLogRotation::appendLogSuffix(
          "foo.log-", FileSuffix(20200302, 123456, 0)),
      "foo.log-20200302.123456");
  EXPECT_EQ(
      TimestampLogRotation::appendLogSuffix(
          "foo.log-", FileSuffix(20200302, 12, 0)),
      "foo.log-20200302.000012");
  EXPECT_EQ(
      TimestampLogRotation::appendLogSuffix("foo.log-", FileSuffix(1, 2, 3)),
      "foo.log-00000001.000002.3");
  EXPECT_EQ(
      TimestampLogRotation::appendLogSuffix(
          "foo.log-", FileSuffix(20200302, 13456, 123)),
      "foo.log-20200302.013456.123");
}

} // namespace facebook::eden
