/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/config/FileChangeMonitor.h"
#include "eden/fs/utils/PathFuncs.h"

using facebook::eden::AbsolutePath;
using facebook::eden::AbsolutePathPiece;
using namespace std::chrono_literals;

namespace {

using facebook::eden::FileChangeMonitor;
using folly::test::TemporaryDirectory;

class MockFileChangeProcessor {
 public:
  MockFileChangeProcessor(bool throwException = false)
      : throwException_{throwException} {}

  MockFileChangeProcessor(const MockFileChangeProcessor&) = delete;
  MockFileChangeProcessor(MockFileChangeProcessor&&) = delete;
  MockFileChangeProcessor& operator=(const MockFileChangeProcessor&) = delete;
  MockFileChangeProcessor& operator=(MockFileChangeProcessor&&) = delete;

  /**
   * Setting the throwException to true will cause exception to be thrown
   * next time the processor is called.
   */
  void setThrowException(bool throwException) {
    throwException_ = throwException;
  }

  void
  operator()(folly::File&& f, int errorNum, AbsolutePathPiece /* unused */) {
    callbackCount_++;
    errorNum_ = errorNum;
    fileContents_ = "";
    fileProcessError_ = false;

    if (throwException_) {
      throw std::invalid_argument("Processed invalid value");
    }

    if (errorNum) {
      return;
    }
    try {
      if (!folly::readFile(f.fd(), fileContents_)) {
        fileProcessError_ = true;
      }
    } catch (const std::exception&) {
      fileProcessError_ = true;
    }
  }
  bool isFileProcessError() {
    return fileProcessError_;
  }
  int getErrorNum() {
    return errorNum_;
  }
  std::string& getFileContents() {
    return fileContents_;
  }
  int getCallbackCount() {
    return callbackCount_;
  }

 private:
  bool throwException_{false};
  int errorNum_{0};
  bool fileProcessError_{false};
  std::string fileContents_{};
  int callbackCount_{0};
};

class FileChangeMonitorTest : public ::testing::Test {
 protected:
  // Top level directory to hold test artifacts
  static constexpr folly::StringPiece fcTestName_{"FileChangeTest"};
  static constexpr folly::StringPiece dataOne_{"this is file one"};
  static constexpr folly::StringPiece dataTwo_{"this is file two"};

  std::unique_ptr<TemporaryDirectory> rootTestDir_;
  AbsolutePath pathOne_;
  AbsolutePath pathTwo_;
  void SetUp() override {
    rootTestDir_ = std::make_unique<TemporaryDirectory>(fcTestName_);
    auto fsPathOne = rootTestDir_->path() / "file.one";
    pathOne_ = AbsolutePath{fsPathOne.string()};
    folly::writeFileAtomic(fsPathOne.string(), dataOne_);

    auto fsPathTwo = rootTestDir_->path() / "file.two";
    pathTwo_ = AbsolutePath{fsPathTwo.string()};
    folly::writeFileAtomic(fsPathTwo.string(), dataTwo_);
  }
  void TearDown() override {
    rootTestDir_.reset();
  }
};
} // namespace
TEST_F(FileChangeMonitorTest, simpleInitTest) {
  MockFileChangeProcessor fcp;
  auto fcm = std::make_shared<FileChangeMonitor>(pathOne_, 200s);

  EXPECT_EQ(fcm->getFilePath(), pathOne_);

  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  EXPECT_FALSE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);
}

TEST_F(FileChangeMonitorTest, nameChangeTest) {
  MockFileChangeProcessor fcp;
  auto fcm = std::make_shared<FileChangeMonitor>(pathOne_, 100s);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), pathOne_);
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  // Changing the file path should force change
  fcm->setFilePath(pathTwo_);
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 2);
  EXPECT_EQ(fcp.getFileContents(), dataTwo_);

  EXPECT_FALSE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 2);
  EXPECT_EQ(fcp.getFileContents(), dataTwo_);

  // Check that the file path was updated
  EXPECT_EQ(fcm->getFilePath(), pathTwo_);
}

TEST_F(FileChangeMonitorTest, noOpNameChangeTest) {
  MockFileChangeProcessor fcp;
  auto fcm = std::make_shared<FileChangeMonitor>(pathOne_, 100s);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), pathOne_);
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  // No-op set of file path - no change!
  fcm->setFilePath(pathOne_);
  EXPECT_FALSE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  // Check that the file path is the same
  EXPECT_EQ(fcm->getFilePath(), pathOne_);
}

TEST_F(FileChangeMonitorTest, modifyExistFileTest) {
  MockFileChangeProcessor fcp;
  auto path =
      AbsolutePath{(rootTestDir_->path() / "ModifyExistFile.txt").string()};
  folly::writeFileAtomic(path.value(), dataOne_);

  auto fcm = std::make_shared<FileChangeMonitor>(path, 0s);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), path.value());
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  folly::writeFileAtomic(path.value(), dataTwo_);

  // File should have changed (there is no throttle)
  EXPECT_EQ(fcm->getFilePath(), path.value());
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 2);
  EXPECT_EQ(fcp.getFileContents(), dataTwo_);
}

TEST_F(FileChangeMonitorTest, fcpMoveTest) {
  MockFileChangeProcessor fcp;
  auto path = AbsolutePath{(rootTestDir_->path() / "FcpMoveTest.txt").string()};
  folly::writeFileAtomic(path.value(), dataOne_);

  auto fcm = std::make_shared<FileChangeMonitor>(path, 0s);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), path.value());
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  folly::writeFileAtomic(path.value(), dataTwo_);

  auto otherFcm = std::move(fcm);
  MockFileChangeProcessor otherFcp;

  // File should have changed (there is no throttle)
  EXPECT_EQ(otherFcm->getFilePath(), path.value());
  EXPECT_TRUE(otherFcm->invokeIfUpdated(std::ref(otherFcp)));
  EXPECT_EQ(otherFcp.getCallbackCount(), 1);
  EXPECT_EQ(otherFcp.getFileContents(), dataTwo_);
}

TEST_F(FileChangeMonitorTest, modifyExistFileThrottleExpiresTest) {
  MockFileChangeProcessor fcp;
  auto path = AbsolutePath{
      (rootTestDir_->path() / "ModifyExistThrottleExpiresTest.txt").string()};
  folly::writeFileAtomic(path.value(), dataOne_);

  auto fcm = std::make_shared<FileChangeMonitor>(path, 10ms);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), path.value());
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  folly::writeFileAtomic(path.value(), dataTwo_);

  auto rslt = fcm->invokeIfUpdated(std::ref(fcp));
  if (!rslt) {
    // The test ran fast (less than 10 millisecond). In this event,
    // check our results (not updated). Then, sleep for a second and validate
    // the update.
    EXPECT_EQ(fcp.getCallbackCount(), 1);
    EXPECT_EQ(fcp.getFileContents(), dataOne_);
    /* sleep override */
    sleep(1);
    rslt = fcm->invokeIfUpdated(std::ref(fcp));
  }
  EXPECT_TRUE(rslt);
  EXPECT_EQ(fcp.getCallbackCount(), 2);
  EXPECT_EQ(fcp.getFileContents(), dataTwo_);
}

TEST_F(FileChangeMonitorTest, modifyExistFileThrottleActiveTest) {
  MockFileChangeProcessor fcp;
  auto path = AbsolutePath{
      (rootTestDir_->path() / "ModifyExistFileThrottleActive.txt").string()};
  folly::writeFileAtomic(path.value(), dataOne_);

  auto fcm = std::make_shared<FileChangeMonitor>(path, 10s);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), path.value());
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  folly::writeFileAtomic(path.value(), dataTwo_);

  // File change throttled
  auto rslt = fcm->invokeIfUpdated(std::ref(fcp));

  EXPECT_FALSE(rslt);
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);
}

TEST_F(FileChangeMonitorTest, nonExistFileTest) {
  MockFileChangeProcessor fcp;
  auto path = AbsolutePath{(rootTestDir_->path() / "NonExist.txt").string()};

  auto fcm = std::make_shared<FileChangeMonitor>(path, 0s);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), path.value());
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getErrorNum(), ENOENT);
}

TEST_F(FileChangeMonitorTest, readFailTest) {
  MockFileChangeProcessor fcp;

  // Note: we are using directory as our path
  auto path = AbsolutePath{rootTestDir_->path().string()};
  auto fcm = std::make_shared<FileChangeMonitor>(path, 0s);

  EXPECT_EQ(fcm->getFilePath(), path.value());
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);

  // Directory can be opened, but read will fail.
  EXPECT_EQ(fcp.getErrorNum(), 0);
  EXPECT_TRUE(fcp.isFileProcessError());
}

TEST_F(FileChangeMonitorTest, rmFileTest) {
  MockFileChangeProcessor fcp;
  auto path =
      AbsolutePath{(rootTestDir_->path() / "ExistToNonExist.txt").string()};
  folly::writeFileAtomic(path.value(), dataOne_);

  auto fcm = std::make_shared<FileChangeMonitor>(path, 0s);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), path.value());
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);

  // Delete file
  remove(path.c_str());

  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 2);
  EXPECT_EQ(fcp.getErrorNum(), ENOENT);
}

TEST_F(FileChangeMonitorTest, processExceptionTest) {
  MockFileChangeProcessor fcp{true};
  auto fcm = std::make_shared<FileChangeMonitor>(pathOne_, 0s);

  // Processor should throw exception on call to invokeIfUpdated
  EXPECT_EQ(fcm->getFilePath(), pathOne_);
  EXPECT_THROW(
      {
        try {
          fcm->invokeIfUpdated(std::ref(fcp));
        } catch (const std::invalid_argument& e) {
          EXPECT_STREQ("Processed invalid value", e.what());
          throw;
        }
      },
      std::invalid_argument);
}

TEST_F(FileChangeMonitorTest, createFileTest) {
  MockFileChangeProcessor fcp;
  auto path =
      AbsolutePath{(rootTestDir_->path() / "NonExistToExist.txt").string()};

  auto fcm = std::make_shared<FileChangeMonitor>(path, 0s);

  // Initial path and change check
  EXPECT_EQ(fcm->getFilePath(), path);
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getErrorNum(), ENOENT);

  // Create the file
  folly::writeFileAtomic(path.value(), dataOne_);

  // File should have changed
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 2);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);
}

TEST_F(FileChangeMonitorTest, openFailTest) {
  // Eden tests are run as root on Sandcastle - which invalidates this test.
  if (getuid() == 0) {
    return;
  }
  MockFileChangeProcessor fcp;
  auto path =
      AbsolutePath{(rootTestDir_->path() / "OpenFailTest.txt").string()};

  // Create the file
  folly::writeFileAtomic(path.value(), dataOne_);
  chmod(path.c_str(), S_IEXEC);

  auto fcm = std::make_shared<FileChangeMonitor>(path, 0s);

  // First time - file changed, but cannot read
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getErrorNum(), EACCES);

  // Nothing changed
  EXPECT_FALSE(fcm->invokeIfUpdated(std::ref(fcp)));

  // Update file - keep permissions same (inaccessible)
  folly::writeFileAtomic(path.value(), dataTwo_);
  EXPECT_EQ(chmod(path.c_str(), S_IEXEC), 0);

  // FileChangeMonitor will not notify if the file has changed AND there is
  // still the same open error.
  EXPECT_FALSE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getErrorNum(), EACCES);
}

TEST_F(FileChangeMonitorTest, openFailFixTest) {
  // Eden tests are run as root on Sandcastle - which invalidates this test.
  if (getuid() == 0) {
    return;
  }

  MockFileChangeProcessor fcp;
  auto path =
      AbsolutePath{(rootTestDir_->path() / "OpenFailFixTest.txt").string()};

  // Create the file
  folly::writeFileAtomic(path.value(), dataOne_);
  EXPECT_EQ(chmod(path.c_str(), S_IEXEC), 0);

  auto fcm = std::make_shared<FileChangeMonitor>(path, 0s);

  // First time - file changed, no read permission
  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 1);
  EXPECT_EQ(fcp.getErrorNum(), EACCES);

  // Fix permissions
  EXPECT_EQ(chmod(path.c_str(), S_IRUSR | S_IRGRP | S_IROTH), 0);

  EXPECT_TRUE(fcm->invokeIfUpdated(std::ref(fcp)));
  EXPECT_EQ(fcp.getCallbackCount(), 2);
  EXPECT_EQ(fcp.getFileContents(), dataOne_);
}
