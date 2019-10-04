/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/win/utils/FileUtils.h"
#include <iostream>
#include <string>
#include "folly/experimental/TestUtil.h"
#include "gtest/gtest.h"

using namespace facebook::eden;
using boost::filesystem::path;
using folly::ByteRange;
using folly::test::TemporaryDirectory;
using folly::test::TemporaryFile;

TEST(FileUtilsTest, testWriteReadFile) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_string();

  std::string writtenContents = "This is the test file.";

  writeFile(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST(FileUtilsTest, testWriteReadFileWide) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_wstring();
  std::string writtenContents = "This is the test file.";

  writeFile(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(fileString.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST(FileUtilsTest, testReadPartialFile) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_string();
  std::string writtenContents =
      "This is the test file. We plan to read the partial contents out of it";

  writeFile(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents, 10);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents.substr(0, 10), readContents);
}

TEST(FileUtilsTest, testReadPartialFileWide) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_wstring();
  std::string writtenContents =
      "This is the test file. We plan to read the partial contents out of it";

  writeFile(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents, 10);
  EXPECT_TRUE(DeleteFile(fileString.c_str()));
  EXPECT_EQ(writtenContents.substr(0, 10), readContents);
}

TEST(FileUtilsTest, testWriteFileAtomicNoTarget) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_string();
  std::string writtenContents = "This is the test file.";

  writeFileAtomic(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST(FileUtilsTest, testWriteFileAtomicNoTargetWide) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_wstring();
  std::string writtenContents = "This is the test file.";

  writeFileAtomic(fileString.c_str(), writtenContents);
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(fileString.c_str()));
  EXPECT_EQ(writtenContents, readContents);
}

TEST(FileUtilsTest, testWriteFileAtomicWithTarget) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
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

TEST(FileUtilsTest, testWriteFileAtomicWithTargetWide) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_wstring();

  // writeFileAtomic takes path with posix path separator.
  std::replace(fileString.begin(), fileString.end(), L'\\', L'/');
  std::string writtenContents1 = "This is the test file.";
  std::string writtenContents2 = "This is new contents.";

  writeFile(fileString.c_str(), writtenContents1);

  writeFileAtomic(fileString.c_str(), writtenContents2);
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(fileString.c_str()));
  EXPECT_EQ(writtenContents2, readContents);
}

TEST(FileUtilsTest, testWriteFileTruncate) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_string();
  std::string writtenContents = "This is the test file.";

  writeFile(fileString.c_str(), std::string("Hello"));
  writeFile(fileString.c_str(), std::string("hi"));
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFileA(fileString.c_str()));
  EXPECT_EQ("hi", readContents);
}

TEST(FileUtilsTest, testWriteFileTruncateWide) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_wstring();
  std::string writtenContents = "This is the test file.";

  writeFile(fileString.c_str(), std::string("Hello"));
  writeFile(fileString.c_str(), std::string("hi"));
  std::string readContents;
  readFile(fileString.c_str(), readContents);
  EXPECT_TRUE(DeleteFile(fileString.c_str()));
  EXPECT_EQ("hi", readContents);
}

TEST(FileUtilsTest, testReadFileFull) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
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

TEST(FileUtilsTest, testReadFileFullWide) {
  TemporaryDirectory tmpDir;
  auto filePath = tmpDir.path() / L"testfile.txt";
  auto fileString = filePath.generic_wstring();

  std::string writtenContents = "This is the test file.";

  writeFile(fileString.c_str(), writtenContents);
  FileHandle fileHandle{CreateFile(
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
  DeleteFile(fileString.c_str());
}

TEST(FileUtilsTest, testGetEnumerationEntries) {
  TemporaryDirectory tmpDir;
  std::string writtenContents = "This is the test file.";

  writeFile(
      path{tmpDir.path() / L"testfile1.txt"}.generic_string().c_str(),
      writtenContents);
  writeFile(
      path{tmpDir.path() / L"testfile2.txt"}.generic_string().c_str(),
      writtenContents);
  writeFile(
      path{tmpDir.path() / L"testfile3.txt"}.generic_string().c_str(),
      writtenContents);
  writeFile(
      path{tmpDir.path() / L"testfile4.txt"}.generic_string().c_str(),
      writtenContents);
  writeFile(
      path{tmpDir.path() / L"testfile5.txt"}.generic_string().c_str(),
      writtenContents);

  CreateDirectoryA(
      path{tmpDir.path() / L"testdir1"}.generic_string().c_str(), nullptr);
  CreateDirectoryA(
      path{tmpDir.path() / L"testdir2"}.generic_string().c_str(), nullptr);

  // Add a directory with a different start letter so we are not always getting
  // all the directory together and then all the files.
  CreateDirectoryA(
      path{tmpDir.path() / L"zztestdir3"}.generic_string().c_str(), nullptr);

  std::vector<DirectoryEntryA> entries =
      getEnumerationEntries(tmpDir.path().generic_string() + "\\*");

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
      getEnumerationEntries(tmpDir.path().generic_wstring() + L"\\*");

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
