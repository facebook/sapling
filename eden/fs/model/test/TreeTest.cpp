/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/String.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"

namespace facebook::eden {

namespace {
std::string testIdHex = folly::to<std::string>(
    "faceb00c",
    "deadbeef",
    "c00010ff",
    "1badb002",
    "8badf00d");

ObjectId testId(testIdHex);
} // namespace

TEST(Tree, testFind) {
  Tree::container entries{CaseSensitivity::Insensitive};
  auto aFileName = PathComponent{"a_file"};
  entries.emplace(aFileName, testId, TreeEntryType::REGULAR_FILE);
  Tree tree(std::move(entries), testId);

  // Verify existent path.
  PathComponentPiece existentPath("a_file");
  auto entry = tree.find(existentPath);
  EXPECT_NE(tree.cend(), entry);
  EXPECT_EQ("a_file", entry->first);
  EXPECT_EQ(false, entry->second.isTree());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, entry->second.getType());

  // Case insensitive testing
  PathComponentPiece existentPath1("A_file");
  entry = tree.find(existentPath1);
  EXPECT_NE(tree.end(), entry);
  EXPECT_EQ("a_file", entry->first);

  PathComponentPiece existentPath2("a_File");
  entry = tree.find(existentPath2);
  EXPECT_NE(tree.end(), entry);
  EXPECT_EQ("a_file", entry->first);

  PathComponentPiece existentPath3("A_FILE");
  entry = tree.find(existentPath3);
  EXPECT_NE(tree.end(), entry);
  EXPECT_EQ("a_file", entry->first);

  // Verify non-existent path.
  PathComponentPiece nonExistentPath("not_a_file");
  EXPECT_EQ(tree.end(), tree.find(nonExistentPath));
}

TEST(Tree, testSize) {
  auto entryType = TreeEntryType::REGULAR_FILE;
  TreeEntry entry{testId, entryType};
  auto entrySize = sizeof(entry);

  auto numEntries = 5;

  Tree::container entries{kPathMapDefaultCaseSensitive};
  for (auto i = 0; i < numEntries; ++i) {
    auto entryName = fmt::format("file{}.txt", i);
    entries.emplace(PathComponentPiece{entryName}, entry);
  }
  Tree tree(std::move(entries), testId);

  // testing the actual size is difficult without just copy pasting the
  // size caalculations, so we are just testing that the size estimate is
  // reasonable. The theortical smallest possible memory footprint is the
  // summ of the footprint of the entrys & the hash
  EXPECT_LE(numEntries * entrySize + Hash20::RAW_SIZE, tree.getSizeBytes());
}

} // namespace facebook::eden
