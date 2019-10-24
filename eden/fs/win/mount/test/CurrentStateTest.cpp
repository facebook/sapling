/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/mount/CurrentState.h"
#include <eden/fs/model/Hash.h>
#include <eden/fs/win/store/WinStore.h>
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/StringConv.h"
#include "gtest/gtest.h"

using namespace facebook::eden;

namespace {
class CurrentStateTest : public ::testing::Test {
 protected:
  void SetUp() override {}

  void TearDown() override {
    auto regPath = std::filesystem::path(rootPath_) / guid_.toWString();
    auto key = RegistryKey::createCurrentUser(regPath.c_str());
    key.deleteKey();
  }

  Guid guid_{Guid::generate()};
  const std::wstring_view rootPath_{L"software\\facebook\\test"};
};
} // namespace

TEST_F(CurrentStateTest, createAndIterateFilesonRoot) {
  auto path = WinRelativePathW(L"dir1\\dir2\\file1");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");

  auto state = CurrentState(rootPath_, guid_.toWString());

  auto metadata = FileMetadata{L"file1.cpp", /* isDir */ false, 10, hash};
  state.entryCreated(L"file1.cpp", metadata);

  metadata = FileMetadata{L"dir1", /* isDir */ true, 0, hash};
  state.entryCreated(L"dir1", metadata);

  state.fileCreated(L"file2.cpp", /* isDirectory */ false);

  metadata = FileMetadata{L"file3.cpp", /* isDir */ false, 30, hash};
  state.entryCreated(L"file3.cpp", metadata);

  metadata = FileMetadata{L"file4.cpp", /* isDir */ false, 40, hash};
  state.entryCreated(L"file4.cpp", metadata);

  state.fileCreated(L"dir2", /*isDir*/ true);

  auto dbNode = state.getDbNode(L"");
  auto entries = dbNode.getDirectoryEntries();
  EXPECT_EQ(entries.size(), 6);

  EXPECT_EQ(entries[0].getName(), L"dir1");
  EXPECT_TRUE(entries[0].isDirectory());
  EXPECT_TRUE(entries[0].hasHash());
  EXPECT_EQ(entries[0].getHash(), hash);
  EXPECT_EQ(entries[0].state(), EntryState::CREATED);

  EXPECT_EQ(entries[1].getName(), L"dir2");
  EXPECT_TRUE(entries[1].isDirectory());
  EXPECT_FALSE(entries[1].hasHash());
  EXPECT_EQ(entries[1].getHash(), Hash{});
  EXPECT_EQ(entries[1].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries[2].getName(), L"file1.cpp");
  EXPECT_FALSE(entries[2].isDirectory());
  EXPECT_TRUE(entries[2].hasHash());
  EXPECT_EQ(entries[2].getHash(), hash);
  EXPECT_EQ(entries[2].state(), EntryState::CREATED);

  EXPECT_EQ(entries[3].getName(), L"file2.cpp");
  EXPECT_FALSE(entries[3].isDirectory());
  EXPECT_FALSE(entries[3].hasHash());
  EXPECT_EQ(entries[3].getHash(), Hash{});
  EXPECT_EQ(entries[3].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries[4].getName(), L"file3.cpp");
  EXPECT_FALSE(entries[4].isDirectory());
  EXPECT_TRUE(entries[4].hasHash());
  EXPECT_EQ(entries[4].getHash(), hash);
  EXPECT_EQ(entries[4].state(), EntryState::CREATED);

  EXPECT_EQ(entries[5].getName(), L"file4.cpp");
  EXPECT_FALSE(entries[5].isDirectory());
  EXPECT_TRUE(entries[5].hasHash());
  EXPECT_EQ(entries[5].getHash(), hash);
  EXPECT_EQ(entries[5].state(), EntryState::CREATED);
}

TEST_F(CurrentStateTest, createAndIterateFilesMultilevel) {
  auto path = WinRelativePathW(L"dir1\\dir2\\file1");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");

  auto state = CurrentState(rootPath_, guid_.toWString());

  auto metadata = FileMetadata{L"file1.cpp", /* isDir */ false, 10, hash};
  state.entryCreated(L"dir1\\dir2\\dir3\\file1.cpp", metadata);

  state.fileCreated(L"dir1\\dir2\\dir3\\dir2", /*isDir*/ true);

  state.fileCreated(L"dir1\\dir2\\dir3\\file2.cpp", /* isDirectory */ false);

  metadata = FileMetadata{L"file3.cpp", /* isDir */ false, 30, hash};
  state.entryCreated(L"dir1\\dir2\\dir3\\file3.cpp", metadata);

  metadata = FileMetadata{L"file11.cpp", /* isDir */ false, 30, hash};
  state.entryCreated(L"dir1\\file11.cpp", metadata);

  state.fileCreated(L"dir1\\dir11", /*isDir*/ true);

  metadata = FileMetadata{L"file4.cpp", /* isDir */ false, 40, hash};
  state.entryCreated(L"dir1\\dir2\\dir3\\file4.cpp", metadata);

  metadata = FileMetadata{L"dir1", /* isDir */ true, 0, hash};
  state.entryCreated(L"dir1\\dir2\\dir3\\dir1", metadata);

  state.fileCreated(L"dir1\\file12.cpp", /* isDirectory */ false);

  auto dbNode1 = state.getDbNode(L"dir1\\dir2\\dir3");
  auto entries1 = dbNode1.getDirectoryEntries();
  EXPECT_EQ(entries1.size(), 6);

  EXPECT_EQ(entries1[0].getName(), L"dir1");
  EXPECT_TRUE(entries1[0].isDirectory());
  EXPECT_TRUE(entries1[0].hasHash());
  EXPECT_EQ(entries1[0].getHash(), hash);
  EXPECT_EQ(entries1[0].state(), EntryState::CREATED);

  EXPECT_EQ(entries1[1].getName(), L"dir2");
  EXPECT_TRUE(entries1[1].isDirectory());
  EXPECT_FALSE(entries1[1].hasHash());
  EXPECT_EQ(entries1[1].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries1[2].getName(), L"file1.cpp");
  EXPECT_FALSE(entries1[2].isDirectory());
  EXPECT_TRUE(entries1[2].hasHash());
  EXPECT_EQ(entries1[2].getHash(), hash);
  EXPECT_EQ(entries1[2].state(), EntryState::CREATED);

  EXPECT_EQ(entries1[3].getName(), L"file2.cpp");
  EXPECT_FALSE(entries1[3].isDirectory());
  EXPECT_FALSE(entries1[3].hasHash());
  EXPECT_EQ(entries1[3].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries1[4].getName(), L"file3.cpp");
  EXPECT_FALSE(entries1[4].isDirectory());
  EXPECT_TRUE(entries1[4].hasHash());
  EXPECT_EQ(entries1[4].getHash(), hash);
  EXPECT_EQ(entries1[4].state(), EntryState::CREATED);

  EXPECT_EQ(entries1[5].getName(), L"file4.cpp");
  EXPECT_FALSE(entries1[5].isDirectory());
  EXPECT_TRUE(entries1[5].hasHash());
  EXPECT_EQ(entries1[5].getHash(), hash);
  EXPECT_EQ(entries1[5].state(), EntryState::CREATED);

  auto dbNode2 = state.getDbNode(L"dir1");
  auto entries2 = dbNode2.getDirectoryEntries();
  EXPECT_EQ(entries2.size(), 4);

  EXPECT_EQ(entries2[0].getName(), L"dir11");
  EXPECT_TRUE(entries2[0].isDirectory());
  EXPECT_FALSE(entries2[0].hasHash());
  EXPECT_EQ(entries2[0].state(), EntryState::MATERIALIZED);

  // Dir2 got created as a part of path creation and will not have any flags
  // set.
  EXPECT_EQ(entries2[1].getName(), L"dir2");

  EXPECT_EQ(entries2[2].getName(), L"file11.cpp");
  EXPECT_FALSE(entries2[2].isDirectory());
  EXPECT_TRUE(entries2[2].hasHash());
  EXPECT_EQ(entries2[2].getHash(), hash);
  EXPECT_EQ(entries2[2].state(), EntryState::CREATED);

  EXPECT_EQ(entries2[3].getName(), L"file12.cpp");
  EXPECT_FALSE(entries2[3].isDirectory());
  EXPECT_FALSE(entries2[3].hasHash());
  EXPECT_EQ(entries2[3].state(), EntryState::MATERIALIZED);
}

TEST_F(CurrentStateTest, stateTransition) {
  auto path = WinRelativePathW(L"dir1\\dir2\\file1");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");

  auto state = CurrentState(rootPath_, guid_.toWString());

  auto metadata = FileMetadata{L"file1.cpp", /* isDir */ false, 10, hash};
  state.entryCreated(L"dir1\\dir2\\dir3\\file1.cpp", metadata);
  state.fileCreated(L"dir1\\dir2\\dir3\\dir1", /*isDir*/ true);
  metadata = FileMetadata{L"file2.cpp", /* isDir */ false, 10, hash};
  state.entryCreated(L"dir1\\dir2\\dir3\\file2.cpp", metadata);
  state.fileCreated(L"dir1\\dir2\\dir3\\file3.cpp", /*IsDirectory*/ false);

  auto dbNode = state.getDbNode(L"dir1\\dir2\\dir3");
  auto entries = dbNode.getDirectoryEntries();
  EXPECT_EQ(entries.size(), 4);

  EXPECT_EQ(entries[0].getName(), L"dir1");
  EXPECT_TRUE(entries[0].isDirectory());
  EXPECT_FALSE(entries[0].hasHash());
  EXPECT_EQ(entries[0].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries[1].getName(), L"file1.cpp");
  EXPECT_FALSE(entries[1].isDirectory());
  EXPECT_TRUE(entries[1].hasHash());
  EXPECT_EQ(entries[1].getHash(), hash);
  EXPECT_EQ(entries[1].state(), EntryState::CREATED);

  EXPECT_EQ(entries[2].getName(), L"file2.cpp");
  EXPECT_FALSE(entries[2].isDirectory());
  EXPECT_TRUE(entries[2].hasHash());
  EXPECT_EQ(entries[2].getHash(), hash);
  EXPECT_EQ(entries[2].state(), EntryState::CREATED);

  EXPECT_EQ(entries[3].getName(), L"file3.cpp");
  EXPECT_FALSE(entries[3].isDirectory());
  EXPECT_FALSE(entries[3].hasHash());
  EXPECT_EQ(entries[3].state(), EntryState::MATERIALIZED);

  state.entryLoaded(L"dir1\\dir2\\dir3\\file1.cpp");
  state.fileRemoved(L"dir1\\dir2\\dir3\\file3.cpp", /*IsDirectory*/ true);
  state.fileRemoved(L"dir1\\dir2\\dir3\\file2.cpp", /*IsDirectory*/ true);

  dbNode = state.getDbNode(L"dir1\\dir2\\dir3");
  entries = dbNode.getDirectoryEntries();
  EXPECT_EQ(entries.size(), 4);

  EXPECT_EQ(entries[0].getName(), L"dir1");
  EXPECT_TRUE(entries[0].isDirectory());
  EXPECT_FALSE(entries[0].hasHash());
  EXPECT_EQ(entries[0].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries[1].getName(), L"file1.cpp");
  EXPECT_FALSE(entries[1].isDirectory());
  EXPECT_TRUE(entries[1].hasHash());
  EXPECT_EQ(entries[1].getHash(), hash);
  EXPECT_EQ(entries[1].state(), EntryState::LOADED);

  EXPECT_EQ(entries[2].getName(), L"file2.cpp");
  EXPECT_FALSE(entries[2].isDirectory());
  EXPECT_TRUE(entries[2].hasHash());
  EXPECT_EQ(entries[2].getHash(), hash);
  EXPECT_EQ(entries[2].state(), EntryState::REMOVED);

  EXPECT_EQ(entries[3].getName(), L"file3.cpp");
  EXPECT_FALSE(entries[3].isDirectory());
  EXPECT_FALSE(entries[3].hasHash());
  EXPECT_EQ(entries[3].state(), EntryState::REMOVED);

  state.fileCreated(L"dir1\\dir2\\dir3\\file2.cpp", /*IsDirectory*/ false);
  state.fileCreated(L"dir1\\dir2\\dir3\\file3.cpp", /*IsDirectory*/ false);
  state.fileModified(L"dir1\\dir2\\dir3\\file1.cpp", /*IsDirectory*/ false);

  dbNode = state.getDbNode(L"dir1\\dir2\\dir3");
  entries = dbNode.getDirectoryEntries();
  EXPECT_EQ(entries.size(), 4);

  EXPECT_EQ(entries[0].getName(), L"dir1");
  EXPECT_TRUE(entries[0].isDirectory());
  EXPECT_FALSE(entries[0].hasHash());
  EXPECT_EQ(entries[0].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries[1].getName(), L"file1.cpp");
  EXPECT_FALSE(entries[1].isDirectory());
  EXPECT_TRUE(entries[1].hasHash());
  EXPECT_EQ(entries[1].getHash(), hash);
  EXPECT_EQ(entries[1].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries[2].getName(), L"file2.cpp");
  EXPECT_FALSE(entries[2].isDirectory());
  EXPECT_FALSE(entries[2].hasHash());
  EXPECT_EQ(entries[2].state(), EntryState::MATERIALIZED);

  EXPECT_EQ(entries[3].getName(), L"file3.cpp");
  EXPECT_FALSE(entries[3].isDirectory());
  EXPECT_FALSE(entries[3].hasHash());
  EXPECT_EQ(entries[3].state(), EntryState::MATERIALIZED);
}
