/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/model/Tree.h"

#include <folly/String.h>
#include <gtest/gtest.h>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/utils/PathFuncs.h"

using facebook::eden::Hash;
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

Hash testHash(testHashHex);
} // namespace

TEST(Tree, testGetEntryPtr) {
  vector<TreeEntry> entries;
  entries.emplace_back(testHash, "a_file", TreeEntryType::REGULAR_FILE);
  Tree tree(std::move(entries));

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
