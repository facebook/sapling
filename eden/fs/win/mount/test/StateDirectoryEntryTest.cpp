/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/mount/StateDirectoryEntry.h"
#include <memory>
#include <string>
#include "eden/fs/model/Hash.h"
#include "eden/fs/win/store/WinStore.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/StringConv.h"
#include "gtest/gtest.h"

using namespace facebook::eden;

TEST(StateDirectoryEntryTest, createDirectoryEntryWithHash) {
  auto parent = std::make_shared<WinRelativePathW>(L"dir1\\dir2\\file1");
  auto name = std::wstring(L"name");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");
  auto info1 =
      StateInfo{EntryState::CREATED, /*IsDirectory*/ false, /*hasHash*/ true};

  auto dirEntry1 = StateDirectoryEntry(parent, name, info1, hash);

  EXPECT_FALSE(dirEntry1.isDirectory());
  EXPECT_EQ(dirEntry1.getName(), name);
  EXPECT_EQ(dirEntry1.getParentPath(), *parent.get());
  EXPECT_EQ(dirEntry1.getHash(), hash);

  auto name1 = name;
  auto info2 =
      StateInfo{EntryState::CREATED, /*IsDirectory*/ true, /*hasHash*/ true};

  auto dirEntry2 = StateDirectoryEntry(parent, std::move(name1), info2, hash);

  EXPECT_TRUE(dirEntry2.isDirectory());
  EXPECT_EQ(dirEntry2.getName(), name);
  EXPECT_EQ(dirEntry2.getParentPath(), *parent.get());
  EXPECT_EQ(dirEntry2.getHash(), hash);
}

TEST(StateDirectoryEntryTest, createDirectoryEntryWithoutHash) {
  auto parent = std::make_shared<WinRelativePathW>(L"dir1\\dir2\\file1");
  auto name = std::wstring(L"name");
  Hash hash;
  auto info1 =
      StateInfo{EntryState::CREATED, /*IsDirectory*/ false, /*hasHash*/ false};

  auto dirEntry1 = StateDirectoryEntry(parent, name, info1);

  EXPECT_FALSE(dirEntry1.isDirectory());
  EXPECT_EQ(dirEntry1.getName(), name);
  EXPECT_EQ(dirEntry1.getParentPath(), *parent.get());
  EXPECT_EQ(dirEntry1.getHash(), hash);

  auto name1 = name;
  auto info2 =
      StateInfo{EntryState::CREATED, /*IsDirectory*/ true, /*hasHash*/ false};

  auto dirEntry2 = StateDirectoryEntry(parent, std::move(name1), info2);

  EXPECT_TRUE(dirEntry2.isDirectory());
  EXPECT_EQ(dirEntry2.getName(), name);
  EXPECT_EQ(dirEntry2.getParentPath(), *parent.get());
  EXPECT_EQ(dirEntry2.getHash(), hash);
}

TEST(StateDirectoryEntryTest, moveDirectoryEntry) {
  auto parent = std::make_shared<WinRelativePathW>(L"dir1\\dir2\\file1");
  auto name = std::wstring(L"name");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");
  auto info =
      StateInfo{EntryState::CREATED, /*isDirectory*/ false, /*hasHash*/ true};

  auto dirEntry1 = StateDirectoryEntry(parent, name, info, hash);

  auto dirEntry2 = StateDirectoryEntry(parent, name, info, hash);

  auto dirEntry3 = StateDirectoryEntry(parent, name, info, hash);

  auto dirEntry4 = std::move(dirEntry1);
  EXPECT_EQ(dirEntry3, dirEntry2);

  auto info2 = StateInfo(
      EntryState::MATERIALIZED, /*isDirectory*/ true, /*hasHash*/ false);
  auto dirEntry5 = StateDirectoryEntry(parent, name, info2);
  EXPECT_NE(dirEntry3, dirEntry5);

  dirEntry5 = std::move(dirEntry2);
  EXPECT_EQ(dirEntry3, dirEntry5);
}

TEST(StateDirectoryEntryTest, compareDirectoryEntry) {
  auto parent = std::make_shared<WinRelativePathW>(L"dir1\\dir2\\file1");
  auto name = std::wstring(L"name");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");
  auto info =
      StateInfo(EntryState::CREATED, /*isDirectory*/ false, /*hasHash*/ true);

  auto dirEntry1 = StateDirectoryEntry(parent, name, info, hash);

  auto dirEntry2 = StateDirectoryEntry(parent, name, info, hash);

  info.hasHash = false;
  auto dirEntry3 = StateDirectoryEntry(parent, name, info);

  EXPECT_EQ(dirEntry1, dirEntry2);
  EXPECT_NE(dirEntry1, dirEntry3);
}
