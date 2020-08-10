/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/utils/FileUtils.h"
#include <filesystem>
#include <string>
#include "eden/fs/win/utils/Guid.h"
#include "gtest/gtest.h"

using namespace facebook::eden;
using std::filesystem::path;

#if 0
namespace {
class FileUtilsTest : public ::testing::Test {
 protected:
  void SetUp() override {
    create_directories(testLocation_);
  }

  void TearDown() override {
    remove_all(testLocation_);
  }

  const path& getTestPath() {
    return testLocation_;
  }
  path testLocation_ =
      std::filesystem::temp_directory_path() / Guid::generate().toWString();
};
} // namespace

TEST_F(FileUtilsTest, testWriteReadFile) {
  auto filePath = getTestPath() / L"testfile.txt";
  auto fileString = filePath.generic_string();

  std::string writtenContents = "This is the test file.";

  writeFile(writtenContents, fileString.c_str());
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST_F(FileUtilsTest, testWriteReadFileWide) {
  auto filePath = getTestPath() / L"testfile.txt";
  std::string writtenContents = "This is the test file.";

  writeFile(writtenContents, filePath.c_str());
  std::string readContents;
  readFile(filePath.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(filePath.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST_F(FileUtilsTest, testReadPartialFile) {
  auto filePath = getTestPath() / L"testfile.txt";
  auto fileString = filePath.generic_string();
  std::string writtenContents =
      "This is the test file. We plan to read the partial contents out of it";

  writeFile(writtenContents, fileString.c_str());
  std::string readContents;
  readFile(fileString.c_str(), readContents, 10);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents.substr(0, 10), readContents);
}

TEST_F(FileUtilsTest, testReadPartialFileWide) {
  auto filePath = getTestPath() / L"testfile.txt";
  std::string writtenContents =
      "This is the test file. We plan to read the partial contents out of it";

  writeFile(writtenContents, filePath.c_str());
  std::string readContents;
  readFile(filePath.c_str(), readContents, 10);
  EXPECT_TRUE(DeleteFile(filePath.c_str()));
  EXPECT_EQ(writtenContents.substr(0, 10), readContents);
}

TEST_F(FileUtilsTest, testWriteFileAtomicNoTarget) {
  auto filePath = getTestPath() / L"testfile.txt";
  auto fileString = filePath.generic_string();
  std::string writtenContents = "This is the test file.";

  writeFileAtomic(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST_F(FileUtilsTest, testWriteFileAtomicNoTargetWide) {
  auto filePath = getTestPath() / L"testfile.txt";
  std::string writtenContents = "This is the test file.";

  writeFileAtomic(filePath.c_str(), writtenContents);
  std::string readContents;
  readFile(filePath.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(filePath.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST_F(FileUtilsTest, testWriteFileAtomicWithTarget) {
  auto filePath = getTestPath() / L"testfile.txt";
  auto fileString = filePath.generic_string();

  // writeFileAtomic takes path with posix path separator.
  std::replace(fileString.begin(), fileString.end(), '\\', '/');
  std::string writtenContents1 = "This is the test file.";
  std::string writtenContents2 = "This is new contents.";

  writeFile(writtenContents1, fileString.c_str());

  writeFileAtomic(fileString.c_str(), writtenContents2);
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents2, readContents);
}

TEST_F(FileUtilsTest, testWriteFileAtomicWithTargetWide) {
  auto filePath = getTestPath() / L"testfile.txt";

  std::string writtenContents1 = "This is the test file.";
  std::string writtenContents2 = "This is new contents.";

  writeFile(writtenContents1, filePath.c_str());

  writeFileAtomic(filePath.c_str(), writtenContents2);
  std::string readContents;
  readFile(filePath.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(filePath.c_str()));
  EXPECT_EQ(writtenContents2, readContents);
}

TEST_F(FileUtilsTest, testWriteFileTruncate) {
  auto filePath = getTestPath() / L"testfile.txt";
  auto fileString = filePath.generic_string();
  std::string writtenContents = "This is the test file.";

  writeFile(std::string("Hello"), fileString.c_str());
  writeFile(std::string("hi"), fileString.c_str());
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ("hi", readContents);
}

TEST_F(FileUtilsTest, testWriteFileTruncateWide) {
  auto filePath = getTestPath() / L"testfile.txt";
  std::string writtenContents = "This is the test file.";

  writeFile(std::string("Hello"), filePath.c_str());
  writeFile(std::string("hi"), filePath.c_str());
  std::string readContents;
  readFile(filePath.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(filePath.c_str()));
  EXPECT_EQ("hi", readContents);
}

TEST_F(FileUtilsTest, testReadFileFull) {
  auto filePath = getTestPath() / L"testfile.txt";
  auto fileString = filePath.generic_string();

  std::string writtenContents = "This is the test file.";

  writeFile(writtenContents, fileString.c_str());
  FileHandle fileHandle{CreateFileA(
      fileString.c_str(),
      GENERIC_READ,
      0,
      nullptr,
      OPEN_ALWAYS,
      FILE_ATTRIBUTE_NORMAL,
      nullptr)};

  char buffer[1024];
  DWORD read = readFile(fileHandle.get(), buffer, 1024);

  EXPECT_EQ(read, writtenContents.size());
  DeleteFileA(fileString.c_str());
}

TEST_F(FileUtilsTest, testReadFileFullWide) {
  auto filePath = getTestPath() / L"testfile.txt";

  std::string writtenContents = "This is the test file.";

  writeFile(writtenContents, filePath.c_str());
  FileHandle fileHandle{CreateFile(
      filePath.c_str(),
      GENERIC_READ,
      0,
      nullptr,
      OPEN_ALWAYS,
      FILE_ATTRIBUTE_NORMAL,
      nullptr)};

  char buffer[1024];
  DWORD read = readFile(fileHandle.get(), buffer, 1024);

  EXPECT_EQ(read, writtenContents.size());
  DeleteFile(filePath.c_str());
}
#endif
