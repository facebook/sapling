/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/String.h>
#include <folly/portability/GTest.h>

#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/PathFuncs.h"

using facebook::eden::Hash20;
using facebook::eden::ObjectId;
using facebook::eden::PathComponent;
using facebook::eden::PathComponentPiece;
using facebook::eden::Tree;
using facebook::eden::TreeEntry;
using facebook::eden::TreeEntryType;
using std::string;
using std::vector;

namespace {
string testHashHex = folly::to<string>(
    "faceb00c",
    "deadbeef",
    "c00010ff",
    "1badb002",
    "8badf00d");

ObjectId testHash(testHashHex);
} // namespace

TEST(Tree, testFind) {
  Tree::container entries;
  auto aFileName = PathComponent{"a_file"};
  entries.emplace_back(
      aFileName, TreeEntry{testHash, TreeEntryType::REGULAR_FILE});
  Tree tree(std::move(entries), testHash);

  // Verify existent path.
  PathComponentPiece existentPath("a_file");
  auto entry = tree.find(existentPath);
  EXPECT_NE(tree.cend(), entry);
  EXPECT_EQ("a_file", entry->first);
  EXPECT_EQ(false, entry->second.isTree());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, entry->second.getType());

#ifdef _WIN32
  // Case insensitive testing only on Windows
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
#endif

  // Verify non-existent path.
  PathComponentPiece nonExistentPath("not_a_file");
  EXPECT_EQ(tree.end(), tree.find(nonExistentPath));
}

TEST(Tree, testSize) {
  std::string entryName{"file.txt"};
  auto entryType = TreeEntryType::REGULAR_FILE;
  TreeEntry entry{testHash, entryType};
  auto entrySize = sizeof(entry);

  auto numEntries = 5;

  Tree::container entries;
  for (auto i = 0; i < numEntries; ++i) {
    entries.emplace_back(entryName, entry);
  }
  Tree tree(std::move(entries), testHash);

  // testing the actual size is diffcult without just copy pasting the
  // size caalculations, so we are just testing that the size estimate is
  // reasonable. The theortical smallest possible memory footprint is the
  // summ of the footprint of the entrys & the hash
  EXPECT_LE(numEntries * entrySize + Hash20::RAW_SIZE, tree.getSizeBytes());
}
