/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

#include "eden/fs/service/PinScanRunner.h"

#include <sys/stat.h>

#include <chrono>
#include <thread>

#include <folly/CancellationToken.h>
#include <folly/FileUtil.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using namespace std::chrono_literals;

namespace {

class PinScanRunnerTest : public ::testing::Test {
 protected:
  /**
   * Write an executable shell script standing in for
   * `edenfs_privhelper --scan-pins` and return its path.
   */
  std::string fakeHelper(const char* name, const std::string& body) {
    auto path = (tmpDir_.path() / name).string();
    folly::writeFileAtomic(path, "#!/bin/sh\n" + body, 0755);
    return path;
  }

  folly::test::TemporaryDirectory tmpDir_;
};

} // namespace

TEST_F(PinScanRunnerTest, returnsTheHelpersReport) {
  auto helper = fakeHelper("ok", "printf 'dev 5\\n5 7\\n5 9\\ndone\\n'\n");

  auto report = runPinScan(helper, folly::CancellationToken{});

  ASSERT_TRUE(report.has_value());
  EXPECT_EQ(1u, report->scannedDevices.count(5));
  const std::vector<uint64_t> expectedPins{7, 9};
  EXPECT_EQ(expectedPins, report->pinsByDevice.at(5));
}

TEST_F(PinScanRunnerTest, failsSafeWhenTheHelperFails) {
  auto helper = fakeHelper("fail", "printf 'dev 5\\n'\nexit 1\n");

  EXPECT_FALSE(runPinScan(helper, folly::CancellationToken{}).has_value());
}

TEST_F(PinScanRunnerTest, stopsWaitingWhenCancelled) {
  auto helper = fakeHelper("hang", "exec sleep 30\n");
  folly::CancellationSource source;
  std::thread canceller([&] {
    std::this_thread::sleep_for(200ms);
    source.requestCancellation();
  });

  auto start = std::chrono::steady_clock::now();
  auto report = runPinScan(helper, source.getToken());
  auto elapsed = std::chrono::steady_clock::now() - start;
  canceller.join();

  EXPECT_FALSE(report.has_value());
  // Well under the 10 second default timeout: the wait ended on
  // cancellation, not on the deadline.
  EXPECT_LT(elapsed, 5s);
}

TEST_F(PinScanRunnerTest, stopsWaitingAtTheDeadline) {
  auto helper = fakeHelper("hang", "exec sleep 30\n");

  auto start = std::chrono::steady_clock::now();
  auto report = runPinScan(helper, folly::CancellationToken{}, 300ms);
  auto elapsed = std::chrono::steady_clock::now() - start;

  EXPECT_FALSE(report.has_value());
  EXPECT_LT(elapsed, 5s);
}

#endif // __linux__
