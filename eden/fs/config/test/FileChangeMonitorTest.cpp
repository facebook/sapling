/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/config/FileChangeMonitor.h"
#include "eden/fs/utils/PathFuncs.h"

using facebook::eden::AbsolutePath;
using facebook::eden::RelativePath;
using namespace std::chrono_literals;

namespace {

using facebook::eden::FileChangeMonitor;
using folly::test::TemporaryDirectory;

class FileChangeMonitorTest : public ::testing::Test {
 protected:
  // Top level directory to hold test artifacts
  static constexpr folly::StringPiece fcTestName_{"FileChangeTest"};
  static constexpr folly::StringPiece dataOne_{"this is file one"};
  static constexpr folly::StringPiece dataTwo_{"this is file two"};

  std::unique_ptr<TemporaryDirectory> rootTestDir_;
  boost::filesystem::path pathOne_;
  boost::filesystem::path pathTwo_;

  void SetUp() override {
    rootTestDir_ = std::make_unique<TemporaryDirectory>(fcTestName_);

    pathOne_ = rootTestDir_->path() / "file.one";
    folly::writeFile(dataOne_, pathOne_.c_str());

    pathTwo_ = rootTestDir_->path() / "file.two";
    folly::writeFile(dataTwo_, pathTwo_.c_str());
  }
  void TearDown() override {
    rootTestDir_.reset();
  }
};
} // namespace
TEST_F(FileChangeMonitorTest, simpleIsChangedTest) {
  auto fcm =
      std::make_shared<FileChangeMonitor>(AbsolutePath{pathOne_.c_str()}, 200s);
  EXPECT_TRUE(fcm->changedSinceUpdate());
  auto filePath = fcm->getFilePath();
  EXPECT_EQ(filePath, pathOne_.c_str());

  // Initial updateStat after FileChangeMonitor creation
  folly::File file(filePath.value());
  fcm->updateStat(file.fd());

  // File did not change after updateStat (ignoreThrottle = TRUE)
  EXPECT_FALSE(fcm->changedSinceUpdate(true));
}

TEST_F(FileChangeMonitorTest, simpleThrottleTest) {
  auto fcm =
      std::make_shared<FileChangeMonitor>(AbsolutePath{pathOne_.c_str()}, 200s);
  EXPECT_TRUE(fcm->changedSinceUpdate());
  auto filePath = fcm->getFilePath();
  EXPECT_EQ(filePath, pathOne_.c_str());

  // Initial updateStat after FileChangeMonitor creation
  folly::File file(filePath.value());
  fcm->updateStat(file.fd());

  // File did not change after updateStat - throttle should kick in
  EXPECT_FALSE(fcm->changedSinceUpdate());

  // File did not change after updateStat (ignoreThrottle = TRUE)
  EXPECT_FALSE(fcm->changedSinceUpdate(true));
}

TEST_F(FileChangeMonitorTest, nameChangeTest) {
  auto fcm =
      std::make_shared<FileChangeMonitor>(AbsolutePath{pathOne_.c_str()}, 10s);
  EXPECT_TRUE(fcm->changedSinceUpdate());
  auto filePath = fcm->getFilePath();
  EXPECT_EQ(filePath, pathOne_.c_str());

  // Initial updateStat after FileChangeMonitor creation
  folly::File file(filePath.value());
  fcm->updateStat(file.fd());

  // File did not change after updateStat (ignoreThrottle = TRUE)
  EXPECT_FALSE(fcm->changedSinceUpdate(true));

  // Changing the file path should force change
  fcm->setFilePath(AbsolutePath(pathTwo_.c_str()));
  EXPECT_TRUE(fcm->changedSinceUpdate(true));

  // Check that the file path was updated
  EXPECT_EQ(fcm->getFilePath(), pathTwo_.c_str());
}

TEST_F(FileChangeMonitorTest, changeTestNoThrottle) {
  auto fcm =
      std::make_shared<FileChangeMonitor>(AbsolutePath{pathOne_.c_str()}, 0s);
  EXPECT_TRUE(fcm->changedSinceUpdate());
  auto filePath = fcm->getFilePath();
  EXPECT_EQ(filePath, pathOne_.c_str());

  // Initial updateStat after FileChangeMonitor creation
  folly::File file(filePath.value());
  fcm->updateStat(file.fd());

  // File did not change after updateStat
  EXPECT_FALSE(fcm->changedSinceUpdate());

  // Make a change to the file. Change will appear immediately (throttle 0s)
  folly::writeFileAtomic(pathOne_.c_str(), dataTwo_);
  EXPECT_TRUE(fcm->changedSinceUpdate());
}

TEST_F(FileChangeMonitorTest, changeThrottleExpireTest) {
  auto fcm =
      std::make_shared<FileChangeMonitor>(AbsolutePath{pathOne_.c_str()}, 1ms);
  EXPECT_TRUE(fcm->changedSinceUpdate());
  auto filePath = fcm->getFilePath();
  EXPECT_EQ(filePath, pathOne_.c_str());

  // Initial updateStat after FileChangeMonitor creation
  folly::File file(filePath.value());
  fcm->updateStat(file.fd());

  // File did not change after updateStat (ignoreThrottle = TRUE)
  EXPECT_FALSE(fcm->changedSinceUpdate(true));

  // Make a change to the file. Change will appear once throttle expires.
  folly::writeFileAtomic(pathOne_.c_str(), dataTwo_);

  auto rslt = fcm->changedSinceUpdate();
  // The test ran fast (less than 1 millisecond). In this unlikely event,
  // sleep for a second and check (rather than allow flaky test)
  if (!rslt) {
    /* sleep override */
    sleep(1);
    rslt = fcm->changedSinceUpdate();
  }
  EXPECT_TRUE(rslt);
}

TEST_F(FileChangeMonitorTest, changeThrottleActiveTest) {
  auto fcm =
      std::make_shared<FileChangeMonitor>(AbsolutePath{pathOne_.c_str()}, 100s);
  EXPECT_TRUE(fcm->changedSinceUpdate());
  auto filePath = fcm->getFilePath();
  EXPECT_EQ(filePath, pathOne_.c_str());

  // Initial updateStat after FileChangeMonitor creation
  folly::File file(filePath.value());
  fcm->updateStat(file.fd());

  // File did not change after updateStat (ignoreThrottle = TRUE)
  EXPECT_FALSE(fcm->changedSinceUpdate(true));

  // Make a change to the file. Change will appear once throttle expires.
  folly::writeFileAtomic(pathOne_.c_str(), dataTwo_);

  // Change only reported with ignoreThrottle = TRUE
  EXPECT_FALSE(fcm->changedSinceUpdate());
  EXPECT_TRUE(fcm->changedSinceUpdate(true));
}

TEST_F(FileChangeMonitorTest, rmFileTest) {
  auto path =
      AbsolutePath{(rootTestDir_->path() / "ExistToNonExist.txt").c_str()};

  folly::writeFileAtomic(path.c_str(), dataOne_);

  auto fcm =
      std::make_shared<FileChangeMonitor>(AbsolutePath{path.c_str()}, 1ms);
  EXPECT_TRUE(fcm->changedSinceUpdate());
  auto filePath = fcm->getFilePath();
  EXPECT_EQ(filePath, path.c_str());

  // Initial updateStat after FileChangeMonitor creation
  folly::File file(filePath.value());
  fcm->updateStat(file.fd());

  // File did not change after updateStat (ignoreThrottle = TRUE)
  EXPECT_FALSE(fcm->changedSinceUpdate(true));

  // Delete file
  remove(path.c_str());

  // File should have changed (ignoreThrottle = TRUE)
  EXPECT_TRUE(fcm->changedSinceUpdate(true));
}

TEST_F(FileChangeMonitorTest, createFileTest) {
  auto path =
      AbsolutePath{(rootTestDir_->path() / "NonExistToExist.txt").c_str()};

  auto fcm =
      std::make_shared<FileChangeMonitor>(AbsolutePath{path.c_str()}, 1ms);
  EXPECT_TRUE(fcm->changedSinceUpdate());
  auto filePath = fcm->getFilePath();
  EXPECT_EQ(filePath, path.c_str());

  // Initial updateStat after FileChangeMonitor creation
  fcm->updateStatWithError(ENOENT);

  // File did not change after updateStat (ignoreThrottle = TRUE)
  EXPECT_FALSE(fcm->changedSinceUpdate(true));

  folly::writeFileAtomic(path.c_str(), dataOne_);

  // File should have changed (ignoreThrottle = TRUE)
  EXPECT_TRUE(fcm->changedSinceUpdate(true));
}
