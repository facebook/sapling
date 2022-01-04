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

TEST(Tree, testGetEntryPtr) {
  vector<TreeEntry> entries;
  entries.emplace_back(
      testHash, PathComponent{"a_file"}, TreeEntryType::REGULAR_FILE);
  Tree tree(std::move(entries), testHash);

  // Verify existent path.
  PathComponentPiece existentPath("a_file");
  auto entry = tree.getEntryPtr(existentPath);
  EXPECT_NE(nullptr, entry);
  EXPECT_EQ("a_file", entry->getName());
  EXPECT_EQ(false, entry->isTree());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, entry->getType());

#ifdef _WIN32
  // Case insensitive testing only on Windows
  PathComponentPiece existentPath1("A_file");
  entry = tree.getEntryPtr(existentPath1);
  EXPECT_NE(nullptr, entry);
  EXPECT_EQ("a_file", entry->getName());

  PathComponentPiece existentPath2("a_File");
  entry = tree.getEntryPtr(existentPath2);
  EXPECT_NE(nullptr, entry);
  EXPECT_EQ("a_file", entry->getName());

  PathComponentPiece existentPath3("A_FILE");
  entry = tree.getEntryPtr(existentPath3);
  EXPECT_NE(nullptr, entry);
  EXPECT_EQ("a_file", entry->getName());
#endif

  // Verify non-existent path.
  PathComponentPiece nonExistentPath("not_a_file");
  EXPECT_EQ(nullptr, tree.getEntryPtr(nonExistentPath));
}

TEST(Tree, testSize) {
  std::string entryName{"file.txt"};
  auto entryType = TreeEntryType::REGULAR_FILE;
  TreeEntry entry{testHash, PathComponent{entryName}, entryType};
  auto entrySize = sizeof(entry) + entry.getIndirectSizeBytes();

  auto numEntries = 5;

  vector<TreeEntry> entries;
  for (auto i = 0; i < numEntries; ++i) {
    entries.emplace_back(entry);
  }
  Tree tree(std::move(entries), testHash);

  // testing the actual size is diffcult without just copy pasting the
  // size caalculations, so we are just testing that the size estimate is
  // reasonable. The theortical smallest possible memory footprint is the
  // summ of the footprint of the entrys & the hash
  EXPECT_LE(numEntries * entrySize + Hash20::RAW_SIZE, tree.getSizeBytes());
}
