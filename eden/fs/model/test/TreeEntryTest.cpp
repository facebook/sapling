/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/TestUtil.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(TreeEntry, modeAndLogString) {
  TreeEntry rwFile(
      makeTestHash("faceb00c"), "file.txt", TreeEntryType::REGULAR_FILE);
  EXPECT_EQ(S_IFREG | 0644, modeFromTreeEntryType(rwFile.getType()));
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, treeEntryTypeFromMode(S_IFREG | 0644));
  EXPECT_EQ(
      "(file.txt, 00000000000000000000000000000000faceb00c, f)",
      rwFile.toLogString());

  TreeEntry rwxFile(
      makeTestHash("789"), "file.exe", TreeEntryType::EXECUTABLE_FILE);
  EXPECT_EQ(S_IFREG | 0755, modeFromTreeEntryType(rwxFile.getType()));
  EXPECT_EQ(
      TreeEntryType::EXECUTABLE_FILE, treeEntryTypeFromMode(S_IFREG | 0755));
  EXPECT_EQ(
      "(file.exe, 0000000000000000000000000000000000000789, x)",
      rwxFile.toLogString());

  TreeEntry rwxLink(makeTestHash("b"), "to-file.exe", TreeEntryType::SYMLINK);
  EXPECT_EQ(S_IFLNK | 0755, modeFromTreeEntryType(rwxLink.getType()));
  EXPECT_EQ(TreeEntryType::SYMLINK, treeEntryTypeFromMode(S_IFLNK | 0755));
  EXPECT_EQ(
      "(to-file.exe, 000000000000000000000000000000000000000b, l)",
      rwxLink.toLogString());

  TreeEntry directory(makeTestHash("abc"), "src", TreeEntryType::TREE);
  EXPECT_EQ(S_IFDIR | 0755, modeFromTreeEntryType(directory.getType()));
  EXPECT_EQ(TreeEntryType::TREE, treeEntryTypeFromMode(S_IFDIR | 0755));
  EXPECT_EQ(
      "(src, 0000000000000000000000000000000000000abc, d)",
      directory.toLogString());

  EXPECT_EQ(std::nullopt, treeEntryTypeFromMode(S_IFSOCK | 0700));
}
