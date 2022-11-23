/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/EdenConfig.h"

#include <folly/experimental/TestUtil.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

#include "eden/fs/config/CachedParsedFileMonitor.h"
#include "eden/fs/model/git/GitIgnore.h"
#include "eden/fs/model/git/GitIgnoreFileParser.h"
#include "eden/fs/utils/FileUtils.h"
#include "eden/fs/utils/PathFuncs.h"

using facebook::eden::AbsolutePath;
using facebook::eden::CachedParsedFileMonitor;
using facebook::eden::GitIgnoreFileParser;
using facebook::eden::writeFile;
using facebook::eden::writeFileAtomic;
using folly::test::TemporaryDirectory;
using namespace std::chrono_literals;
using namespace facebook::eden;

namespace {
static constexpr folly::StringPiece kErrorFileContents{"THROW ERROR:"};

/**
 * A simple FileParser for test purposes. It reads in the entire file into a
 * string. If the file contents are of form "THROW ERROR:INT", the parse
 * result will be ERROR_NUM.
 */
class TestFileParser {
 public:
  using value_type = std::string;

  /**
   * Parse entire file into a string.
   * @return the string on success or non-zero error code on failure.
   */
  folly::Expected<std::string, int> operator()(
      int fileDescriptor,
      facebook::eden::AbsolutePathPiece filePath) const {
    try {
      std::string fileContents;
      auto in = folly::File(fileDescriptor); // throws if file does not exist
      if (!folly::readFile(in.fd(), fileContents)) {
        return folly::makeUnexpected<int>((int)errno);
      }

      if (fileContents.find(kErrorFileContents.str()) == 0) {
        auto errorString = fileContents.substr(kErrorFileContents.size());
        auto errorNum = std::stoi(errorString);
        return folly::makeUnexpected<int>((int)errorNum);
      }

      return fileContents;
    } catch (const std::system_error& ex) {
      XLOG(WARNING) << "error reading file " << filePath
                    << folly::exceptionStr(ex);
      return folly::makeUnexpected<int>((int)errno);
    } catch (const std::exception& ex) {
      XLOG(WARNING) << "error reading file " << filePath
                    << folly::exceptionStr(ex);
      return folly::makeUnexpected<int>((int)errno);
    }
  }
};

class CachedParsedFileMonitorTest : public ::testing::Test {
 protected:
  // Top level directory to hold test artifacts
  static constexpr folly::StringPiece fcTestName_{"FileChangeTest"};
  std::unique_ptr<TemporaryDirectory> rootTestDir_;
  AbsolutePath rootPath_;

  static constexpr folly::StringPiece dataOne_{"this is file one"};
  static constexpr folly::StringPiece dataTwo_{"this is file two"};
  static constexpr int invalidParseErrorCode_{99};
  const std::string invalidParseDataOne_{
      kErrorFileContents.str() +
      folly::to<std::string>(invalidParseErrorCode_)};
  static constexpr folly::StringPiece gitIgnoreDataOne_ = R"(
*.com
*.class
*.dll
*.exe
*.o
*.so)";
  AbsolutePath pathOne_;
  AbsolutePath pathTwo_;
  AbsolutePath invalidParsePathOne_;
  AbsolutePath gitIgnorePathOne_;
  AbsolutePath bogusPathOne_;
  AbsolutePath bogusPathTwo_;
  void SetUp() override {
    rootTestDir_ = std::make_unique<TemporaryDirectory>(fcTestName_);
    rootPath_ = canonicalPath(rootTestDir_->path().string());

    pathOne_ = rootPath_ + "file.one"_pc;
    writeFile(pathOne_, dataOne_).throwUnlessValue();

    pathTwo_ = rootPath_ + "file.two"_pc;
    writeFile(pathTwo_, dataTwo_).throwUnlessValue();

    invalidParsePathOne_ = rootPath_ + "invalidParse.one"_pc;
    writeFile(invalidParsePathOne_, folly::StringPiece(invalidParseDataOne_))
        .throwUnlessValue();

    gitIgnorePathOne_ = rootPath_ + "gitignore.one"_pc;
    writeFile(gitIgnorePathOne_, gitIgnoreDataOne_).throwUnlessValue();

    bogusPathOne_ = rootPath_ + "THIS_IS_BOGUS"_pc;
  }
  void TearDown() override {
    rootTestDir_.reset();
  }
}; // namespace
} // namespace

TEST_F(CachedParsedFileMonitorTest, baseIsChangedTest) {
  auto fcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(pathOne_, 0s);

  // Check the correct file data is returned
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);
}

TEST_F(CachedParsedFileMonitorTest, updateNameTest) {
  auto fcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(pathOne_, 0s);

  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // If we ask for a different file, we should get updated file contents
  // immediately. This is true, even though we have a throttle.
  rslt = fcm->getFileContents(pathTwo_);
  EXPECT_EQ(rslt.value(), dataTwo_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents(pathTwo_);
  EXPECT_EQ(rslt.value(), dataTwo_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);
}

TEST_F(CachedParsedFileMonitorTest, fileDoesNotExist) {
  auto fcm = std::make_shared<CachedParsedFileMonitor<TestFileParser>>(
      bogusPathOne_, 0s);
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), ENOENT);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), ENOENT);
  EXPECT_EQ(fcm->getUpdateCount(), 1);
}

TEST_F(CachedParsedFileMonitorTest, updateNameToFileNonExistToExist) {
  auto fcm = std::make_shared<CachedParsedFileMonitor<TestFileParser>>(
      bogusPathOne_, 0s);
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), ENOENT);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // Different file name, we should see the updated file contents immediately.
  rslt = fcm->getFileContents(pathOne_);
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);
}

TEST_F(CachedParsedFileMonitorTest, updateNameFileExistToNonExist) {
  auto fcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(pathOne_, 0s);

  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // If we ask for a different file (that does not exist) we should get
  // an error code immediately.
  rslt = fcm->getFileContents(bogusPathOne_);
  EXPECT_EQ(rslt.error(), ENOENT);
  EXPECT_EQ(fcm->getUpdateCount(), 2);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), ENOENT);
  EXPECT_EQ(fcm->getUpdateCount(), 2);
}

TEST_F(CachedParsedFileMonitorTest, updateFileNonExistToExist) {
  auto path = rootPath_ + "NonExistToExist.txt"_pc;
  auto fcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(path, 0s);
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), ENOENT);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // Over-write data in file with valid data
  writeFileAtomic(path, dataOne_).throwUnlessValue();

  // We should see the updated results (no throttle)
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);
}

TEST_F(CachedParsedFileMonitorTest, updateFileExistToNonExist) {
  auto path = rootPath_ + "ExistToNonExist.txt"_pc;

  // Create a test file that we will subsequently delete
  writeFileAtomic(path, dataOne_).throwUnlessValue();

  auto fcm = std::make_shared<CachedParsedFileMonitor<TestFileParser>>(
      canonicalPath(path.c_str()), 0s);
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // Delete file
  remove(path.c_str());

  // We should see the updated results (no throttle)
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), ENOENT);
  EXPECT_EQ(fcm->getUpdateCount(), 2);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), ENOENT);
  EXPECT_EQ(fcm->getUpdateCount(), 2);
}

TEST_F(CachedParsedFileMonitorTest, fileParseError) {
  auto fcm = std::make_shared<CachedParsedFileMonitor<TestFileParser>>(
      invalidParsePathOne_, 10s);
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), invalidParseErrorCode_);
}

TEST_F(CachedParsedFileMonitorTest, updateFileParseErrorToNoError) {
  auto fcm = std::make_shared<CachedParsedFileMonitor<TestFileParser>>(
      invalidParsePathOne_, 0s);
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), invalidParseErrorCode_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // Over-write data in file with valid data
  writeFileAtomic(invalidParsePathOne_, dataOne_).throwUnlessValue();

  // We should see the updated results (no throttle)
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);

  // Over-write data in file with invalid data
  writeFileAtomic(
      invalidParsePathOne_, folly::StringPiece{invalidParseDataOne_})
      .throwUnlessValue();

  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), invalidParseErrorCode_);
  EXPECT_EQ(fcm->getUpdateCount(), 3);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), invalidParseErrorCode_);
  EXPECT_EQ(fcm->getUpdateCount(), 3);
}

TEST_F(CachedParsedFileMonitorTest, updateNoErrorToFileParseError) {
  auto path = rootPath_ + "UpdateNoErrorToError.txt"_pc;

  // Create file with valid data
  writeFileAtomic(path, dataOne_).throwUnlessValue();

  auto fcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(path, 0s);
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // Over-write data in file with invalid data
  writeFileAtomic(path, folly::StringPiece{invalidParseDataOne_})
      .throwUnlessValue();

  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), invalidParseErrorCode_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);

  // Make sure same results - and no reload
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), invalidParseErrorCode_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);
}

#ifndef _WIN32
TEST_F(CachedParsedFileMonitorTest, modifyThrottleTest) {
  auto path = rootPath_ + "modifyThrottleTest.txt"_pc;

  // Create file with valid data
  writeFileAtomic(path, dataOne_).throwUnlessValue();

  auto fcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(path, 10s);

  // Create a new CachedParsedFileMonitor and we will see the updates.
  auto noThrottleFcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(path, 0s);

  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  auto noThrottleRslt = noThrottleFcm->getFileContents();
  EXPECT_EQ(noThrottleRslt.value(), dataOne_);
  EXPECT_EQ(noThrottleFcm->getUpdateCount(), 1);

  // Over-write data in file
  writeFileAtomic(path, dataTwo_).throwUnlessValue();

  // Throttle does not see results
  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  // No throttle should see the results
  noThrottleRslt = noThrottleFcm->getFileContents();
  EXPECT_EQ(noThrottleRslt.value(), dataTwo_);
  EXPECT_EQ(noThrottleFcm->getUpdateCount(), 2);
}

TEST_F(CachedParsedFileMonitorTest, modifyTest) {
  auto path = rootPath_ + "modifyTest.txt"_pc;

  // Create file with valid data
  writeFileAtomic(path, dataOne_).throwUnlessValue();

  auto fcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(path, 10ms);

  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  writeFileAtomic(path, dataTwo_).throwUnlessValue();
  // Over-write data in file
  // Sleep over our throttle. We could increase sleep time if the o/s sleep
  // is not accurate enough (and we are seeing false positives).
  /* sleep override */
  sleep(1);

  rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataTwo_);
  EXPECT_EQ(fcm->getUpdateCount(), 2);
}

TEST_F(CachedParsedFileMonitorTest, moveTest) {
  auto path = rootPath_ + "moveTest.txt"_pc;

  // Create file with valid data
  writeFileAtomic(path, dataOne_).throwUnlessValue();

  auto fcm =
      std::make_shared<CachedParsedFileMonitor<TestFileParser>>(path, 0s);

  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataOne_);
  EXPECT_EQ(fcm->getUpdateCount(), 1);

  auto otherFcm = std::move(fcm);

  writeFileAtomic(path, dataTwo_).throwUnlessValue();

  rslt = otherFcm->getFileContents();
  EXPECT_EQ(rslt.value(), dataTwo_);
  EXPECT_EQ(otherFcm->getUpdateCount(), 2);
}
#endif

TEST_F(CachedParsedFileMonitorTest, gitParserTest) {
  auto fcm = std::make_shared<CachedParsedFileMonitor<GitIgnoreFileParser>>(
      gitIgnorePathOne_, 10s);

  // Check the correct file data is returned
  auto rslt = fcm->getFileContents();
  EXPECT_FALSE(rslt.value().empty());
}

TEST_F(CachedParsedFileMonitorTest, gitParserEmptyTest) {
  auto fcm = std::make_shared<CachedParsedFileMonitor<GitIgnoreFileParser>>(
      bogusPathOne_, 10s);

  // Check the correct file data is returned
  auto rslt = fcm->getFileContents();
  EXPECT_EQ(rslt.error(), ENOENT);
}
