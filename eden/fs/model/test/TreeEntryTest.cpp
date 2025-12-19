/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <gtest/gtest.h>
#include <optional>

#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/EdenError.h"

using namespace facebook::eden;

TEST(TreeEntry, modeAndLogString) {
  TreeEntry rwFile(makeTestId("faceb00c"), TreeEntryType::REGULAR_FILE);
  EXPECT_EQ(S_IFREG | 0644, modeFromTreeEntryType(rwFile.getType()));
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, treeEntryTypeFromMode(S_IFREG | 0644));
  EXPECT_EQ(
      "(file.txt, 00000000000000000000000000000000faceb00c, f)",
      rwFile.toLogString("file.txt"_pc));

  TreeEntry rwxFile(makeTestId("789"), TreeEntryType::EXECUTABLE_FILE);
  EXPECT_EQ(S_IFREG | 0755, modeFromTreeEntryType(rwxFile.getType()));
  EXPECT_EQ(
      TreeEntryType::EXECUTABLE_FILE, treeEntryTypeFromMode(S_IFREG | 0755));
  EXPECT_EQ(
      "(file.exe, 0000000000000000000000000000000000000789, x)",
      rwxFile.toLogString("file.exe"_pc));

  TreeEntry rwxLink(makeTestId("b"), TreeEntryType::SYMLINK);
  EXPECT_EQ(S_IFLNK | 0755, modeFromTreeEntryType(rwxLink.getType()));
  EXPECT_EQ(TreeEntryType::SYMLINK, treeEntryTypeFromMode(S_IFLNK | 0755));
  EXPECT_EQ(
      "(to-file.exe, 000000000000000000000000000000000000000b, l)",
      rwxLink.toLogString("to-file.exe"_pc));

  TreeEntry directory(makeTestId("abc"), TreeEntryType::TREE);
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
  TreeEntry rwFile{makeTestId("faceb00c"), type};

  auto sizeofSize = sizeof(rwFile);
  auto totalSize = sizeofSize;

  EXPECT_LE(Hash20::RAW_SIZE + sizeof(TreeEntryType), totalSize);
}

TEST(TreeEntry, testEntryAttributesEqual) {
  EntryAttributes nullAttributes{
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt};
  EntryAttributes error1Attributes{
      std::nullopt,
      std::nullopt,
      folly::Try<uint64_t>{newEdenError(std::exception{})},
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt};
  EntryAttributes error2Attributes{
      std::nullopt,
      std::nullopt,
      folly::Try<uint64_t>{
          newEdenError(std::runtime_error{"some other error"})},
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt};
  EntryAttributes real1Attributes{
      std::nullopt,
      std::nullopt,
      folly::Try<uint64_t>{1},
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt};
  EntryAttributes real2Attributes{
      std::nullopt,
      std::nullopt,
      folly::Try<uint64_t>{2},
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt};

  EXPECT_EQ(nullAttributes, nullAttributes);
  EXPECT_NE(nullAttributes, error1Attributes);
  EXPECT_EQ(error1Attributes, error2Attributes);
  EXPECT_NE(nullAttributes, real1Attributes);
  EXPECT_NE(real1Attributes, real2Attributes);
  EXPECT_EQ(real1Attributes, real1Attributes);
}

TEST(TreeEntry, filteredEntryType) {
  if (folly::kIsWindows) {
    // On windows, symlinks should be preserved if windowsSymlinksEnabled is
    // true, and converted to regular files if windowsSymlinksEnabled is false
    EXPECT_EQ(
        TreeEntryType::SYMLINK,
        filteredEntryType(TreeEntryType::SYMLINK, true));
    EXPECT_EQ(
        TreeEntryType::REGULAR_FILE,
        filteredEntryType(TreeEntryType::SYMLINK, false));
  } else {
    // On non-windows, symlinks should be preserved regardless of
    // windowsSymlinksEnabled
    EXPECT_EQ(
        TreeEntryType::SYMLINK,
        filteredEntryType(TreeEntryType::SYMLINK, true));
    EXPECT_EQ(
        TreeEntryType::SYMLINK,
        filteredEntryType(TreeEntryType::SYMLINK, false));
  }

  // Other than symlinks, the type should be preserved regardless of
  // windowsSymlinksEnabled
  for (auto type :
       {TreeEntryType::TREE,
        TreeEntryType::REGULAR_FILE,
        TreeEntryType::EXECUTABLE_FILE}) {
    EXPECT_EQ(type, filteredEntryType(type, true));
    EXPECT_EQ(type, filteredEntryType(type, false));
  }
}
TEST(TreeEntry, compareTreeEntryType) {
  // Test that identical types compare as equal
  EXPECT_TRUE(compareTreeEntryType(
      TreeEntryType::REGULAR_FILE, TreeEntryType::REGULAR_FILE));
  EXPECT_TRUE(compareTreeEntryType(
      TreeEntryType::EXECUTABLE_FILE, TreeEntryType::EXECUTABLE_FILE));
  EXPECT_TRUE(
      compareTreeEntryType(TreeEntryType::SYMLINK, TreeEntryType::SYMLINK));
  EXPECT_TRUE(compareTreeEntryType(TreeEntryType::TREE, TreeEntryType::TREE));

  // Test that different types compare as not equal
  EXPECT_FALSE(compareTreeEntryType(
      TreeEntryType::REGULAR_FILE, TreeEntryType::SYMLINK));
  EXPECT_FALSE(
      compareTreeEntryType(TreeEntryType::REGULAR_FILE, TreeEntryType::TREE));
  EXPECT_FALSE(compareTreeEntryType(
      TreeEntryType::EXECUTABLE_FILE, TreeEntryType::SYMLINK));
  EXPECT_FALSE(compareTreeEntryType(
      TreeEntryType::EXECUTABLE_FILE, TreeEntryType::TREE));
  EXPECT_FALSE(
      compareTreeEntryType(TreeEntryType::SYMLINK, TreeEntryType::TREE));

  if (folly::kIsWindows) {
    // On Windows REGULAR_FILE and EXECUTABLE_FILE types should consider equal
    EXPECT_TRUE(compareTreeEntryType(
        TreeEntryType::REGULAR_FILE, TreeEntryType::EXECUTABLE_FILE));
  } else {
    EXPECT_FALSE(compareTreeEntryType(
        TreeEntryType::REGULAR_FILE, TreeEntryType::EXECUTABLE_FILE));
  }
}
