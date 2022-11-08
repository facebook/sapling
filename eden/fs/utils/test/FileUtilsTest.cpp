/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FileUtils.h"
#include <folly/Range.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <string>
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;
using testing::UnorderedElementsAre;
using folly::literals::string_piece_literals::operator""_sp;

namespace {
class FileUtilsTest : public ::testing::Test {
 protected:
  void SetUp() override {
    tempDir_ = makeTempDir();
    testLocation_ = AbsolutePath(canonicalPath(tempDir_.path().string()));
  }

  const AbsolutePathPiece getTestPath() {
    return testLocation_;
  }
  folly::test::TemporaryDirectory tempDir_;
  AbsolutePath testLocation_;
};
} // namespace

TEST_F(FileUtilsTest, testWriteReadFile) {
  auto filePath = getTestPath() + "testfile.txt"_pc;

  auto writtenContent = "This is the test file."_sp;

  writeFile(filePath, writtenContent).value();
  auto readContents = readFile(filePath).value();
  EXPECT_EQ(writtenContent, readContents);
}

TEST_F(FileUtilsTest, testReadPartialFile) {
  auto filePath = getTestPath() + "testfile.txt"_pc;
  auto writtenContent =
      "This is the test file. We plan to read the partial contents out of it"_sp;

  writeFile(filePath, writtenContent).value();
  std::string readContents = readFile(filePath, 10).value();
  EXPECT_EQ(writtenContent.subpiece(0, 10), readContents);
}

TEST_F(FileUtilsTest, testWriteFileAtomicNoTarget) {
  auto filePath = getTestPath() + "testfile.txt"_pc;
  auto writtenContent = "This is the test file."_sp;

  writeFileAtomic(filePath, writtenContent).value();
  std::string readContents = readFile(filePath).value();
  EXPECT_EQ(writtenContent, readContents);
}

TEST_F(FileUtilsTest, testWriteFileAtomicWithTarget) {
  auto filePath = getTestPath() + "testfile.txt"_pc;

  auto writtenContents1 = "This is the test file."_sp;
  auto writtenContents2 = "This is new contents."_sp;

  writeFile(filePath, writtenContents1).value();
  writeFileAtomic(filePath, writtenContents2).value();

  std::string readContents = readFile(filePath).value();
  EXPECT_EQ(writtenContents2, readContents);
}

TEST_F(FileUtilsTest, testWriteFileTruncate) {
  auto filePath = getTestPath() + "testfile.txt"_pc;

  writeFile(filePath, "Hello"_sp).value();
  writeFile(filePath, "hi"_sp).value();
  std::string readContents = readFile(filePath).value();
  EXPECT_EQ("hi", readContents);
}

TEST_F(FileUtilsTest, testGetAllDirectoryEntryNames) {
  writeFile(getTestPath() + "A"_pc, "A"_sp).value();
  writeFile(getTestPath() + "B"_pc, "B"_sp).value();
  writeFile(getTestPath() + "C"_pc, "C"_sp).value();
  writeFile(getTestPath() + "D"_pc, "D"_sp).value();
  writeFile(getTestPath() + "E"_pc, "E"_sp).value();
  writeFile(getTestPath() + "ABCDEF"_pc, "ACBDEF"_sp).value();

  auto direntNames = getAllDirectoryEntryNames(getTestPath()).value();
  EXPECT_THAT(
      direntNames,
      UnorderedElementsAre(
          "A"_pc, "B"_pc, "C"_pc, "D"_pc, "E"_pc, "ABCDEF"_pc));
}
