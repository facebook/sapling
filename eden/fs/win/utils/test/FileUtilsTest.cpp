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

  writeFile(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST_F(FileUtilsTest, testWriteReadFileWide) {
  auto filePath = getTestPath() / L"testfile.txt";
  std::string writtenContents = "This is the test file.";

  writeFile(filePath.c_str(), writtenContents);
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

  writeFile(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents, 10);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents.substr(0, 10), readContents);
}

TEST_F(FileUtilsTest, testReadPartialFileWide) {
  auto filePath = getTestPath() / L"testfile.txt";
  std::string writtenContents =
      "This is the test file. We plan to read the partial contents out of it";

  writeFile(filePath.c_str(), writtenContents);
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

  writeFile(fileString.c_str(), writtenContents1);

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

  writeFile(filePath.c_str(), writtenContents1);

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

  writeFile(fileString.c_str(), std::string("Hello"));
  writeFile(fileString.c_str(), std::string("hi"));
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ("hi", readContents);
}

TEST_F(FileUtilsTest, testWriteFileTruncateWide) {
  auto filePath = getTestPath() / L"testfile.txt";
  std::string writtenContents = "This is the test file.";

  writeFile(filePath.c_str(), std::string("Hello"));
  writeFile(filePath.c_str(), std::string("hi"));
  std::string readContents;
  readFile(filePath.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(filePath.c_str()));
  EXPECT_EQ("hi", readContents);
}

TEST_F(FileUtilsTest, testReadFileFull) {
  auto filePath = getTestPath() / L"testfile.txt";
  auto fileString = filePath.generic_string();

  std::string writtenContents = "This is the test file.";

  writeFile(fileString.c_str(), writtenContents);
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

  writeFile(filePath.c_str(), writtenContents);
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

TEST_F(FileUtilsTest, testGetEnumerationEntries) {
  std::string writtenContents = "This is the test file.";

  writeFile(path{getTestPath() / L"testfile1.txt"}.c_str(), writtenContents);
  writeFile(path{getTestPath() / L"testfile2.txt"}.c_str(), writtenContents);
  writeFile(path{getTestPath() / L"testfile3.txt"}.c_str(), writtenContents);
  writeFile(path{getTestPath() / L"testfile4.txt"}.c_str(), writtenContents);
  writeFile(path{getTestPath() / L"testfile5.txt"}.c_str(), writtenContents);

  create_directory(path{getTestPath() / L"testdir1"});
  create_directory(path{getTestPath() / L"testdir2"});

  // Add a directory with a different start letter so we are not always getting
  // all the directory together and then all the files.
  create_directory(path{getTestPath() / L"zztestdir3"});

  std::vector<DirectoryEntryA> entries =
      getEnumerationEntries(path{getTestPath() / L"*"}.generic_string());

  EXPECT_EQ(entries.size(), 8);
  EXPECT_EQ(std::string(entries[0].data.cFileName), "testdir1");
  EXPECT_EQ(std::string(entries[1].data.cFileName), "testdir2");
  EXPECT_EQ(std::string(entries[2].data.cFileName), "testfile1.txt");
  EXPECT_EQ(std::string(entries[3].data.cFileName), "testfile2.txt");
  EXPECT_EQ(std::string(entries[4].data.cFileName), "testfile3.txt");
  EXPECT_EQ(std::string(entries[5].data.cFileName), "testfile4.txt");
  EXPECT_EQ(std::string(entries[6].data.cFileName), "testfile5.txt");
  EXPECT_EQ(std::string(entries[7].data.cFileName), "zztestdir3");

  std::vector<DirectoryEntryW> entriesWide =
      getEnumerationEntries(getTestPath() / L"*");

  EXPECT_EQ(entriesWide.size(), 8);
  EXPECT_EQ(std::wstring(entriesWide[0].data.cFileName), L"testdir1");
  EXPECT_EQ(std::wstring(entriesWide[1].data.cFileName), L"testdir2");
  EXPECT_EQ(std::wstring(entriesWide[2].data.cFileName), L"testfile1.txt");
  EXPECT_EQ(std::wstring(entriesWide[3].data.cFileName), L"testfile2.txt");
  EXPECT_EQ(std::wstring(entriesWide[4].data.cFileName), L"testfile3.txt");
  EXPECT_EQ(std::wstring(entriesWide[5].data.cFileName), L"testfile4.txt");
  EXPECT_EQ(std::wstring(entriesWide[6].data.cFileName), L"testfile5.txt");
  EXPECT_EQ(std::wstring(entriesWide[7].data.cFileName), L"zztestdir3");
}
