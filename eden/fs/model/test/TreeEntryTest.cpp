/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GTest.h>

#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;

TEST(TreeEntry, modeAndLogString) {
  TreeEntry rwFile(makeTestHash("faceb00c"), TreeEntryType::REGULAR_FILE);
  EXPECT_EQ(S_IFREG | 0644, modeFromTreeEntryType(rwFile.getType()));
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, treeEntryTypeFromMode(S_IFREG | 0644));
  EXPECT_EQ(
      "(file.txt, 00000000000000000000000000000000faceb00c, f)",
      rwFile.toLogString("file.txt"_pc));

  TreeEntry rwxFile(makeTestHash("789"), TreeEntryType::EXECUTABLE_FILE);
#ifndef _WIN32
  EXPECT_EQ(S_IFREG | 0755, modeFromTreeEntryType(rwxFile.getType()));
  EXPECT_EQ(
      TreeEntryType::EXECUTABLE_FILE, treeEntryTypeFromMode(S_IFREG | 0755));
#endif
  EXPECT_EQ(
      "(file.exe, 0000000000000000000000000000000000000789, x)",
      rwxFile.toLogString("file.exe"_pc));

  TreeEntry rwxLink(makeTestHash("b"), TreeEntryType::SYMLINK);
#ifndef _WIN32
  EXPECT_EQ(S_IFLNK | 0755, modeFromTreeEntryType(rwxLink.getType()));
  EXPECT_EQ(TreeEntryType::SYMLINK, treeEntryTypeFromMode(S_IFLNK | 0755));
#endif
  EXPECT_EQ(
      "(to-file.exe, 000000000000000000000000000000000000000b, l)",
      rwxLink.toLogString("to-file.exe"_pc));

  TreeEntry directory(makeTestHash("abc"), TreeEntryType::TREE);
  EXPECT_EQ(S_IFDIR | 0755, modeFromTreeEntryType(directory.getType()));
  EXPECT_EQ(TreeEntryType::TREE, treeEntryTypeFromMode(S_IFDIR | 0755));
  EXPECT_EQ(
      "(src, 0000000000000000000000000000000000000abc, d)",
      directory.toLogString("src"_pc));

#ifndef _WIN32
  EXPECT_EQ(std::nullopt, treeEntryTypeFromMode(S_IFSOCK | 0700));
#endif
}

TEST(TreeEntry, testEntrySize) {
  auto type = TreeEntryType::REGULAR_FILE;
  TreeEntry rwFile{makeTestHash("faceb00c"), type};

  auto sizeofSize = sizeof(rwFile);
  auto totalSize = sizeofSize;

  EXPECT_LE(Hash20::RAW_SIZE + sizeof(TreeEntryType), totalSize);
}
