/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GTest.h>

#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/EdenError.h"

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

TEST(TreeEntry, testEntryAttributesEqual) {
  EntryAttributes nullAttributes{std::nullopt, std::nullopt, std::nullopt};
  EntryAttributes error1Attributes{
      std::nullopt,
      folly::Try<uint64_t>{newEdenError(std::exception{})},
      std::nullopt};
  EntryAttributes error2Attributes{
      std::nullopt,
      folly::Try<uint64_t>{
          newEdenError(std::runtime_error{"some other error"})},
      std::nullopt};
  EntryAttributes real1Attributes{
      std::nullopt, folly::Try<uint64_t>{1}, std::nullopt};
  EntryAttributes real2Attributes{
      std::nullopt, folly::Try<uint64_t>{2}, std::nullopt};

  EXPECT_EQ(nullAttributes, nullAttributes);
  EXPECT_NE(nullAttributes, error1Attributes);
  EXPECT_EQ(error1Attributes, error2Attributes);
  EXPECT_NE(nullAttributes, real1Attributes);
  EXPECT_NE(real1Attributes, real2Attributes);
  EXPECT_EQ(real1Attributes, real1Attributes);
}
