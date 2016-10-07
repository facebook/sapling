/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/Tree.h"

#include <folly/String.h>
#include <gtest/gtest.h>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/utils/PathFuncs.h"

using facebook::eden::Hash;
using facebook::eden::FileType;
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
}

TEST(Tree, testGetEntryPtr) {
  uint8_t mode = 0b110;
  vector<TreeEntry> entries;
  entries.emplace_back(testHash, "a_file", FileType::REGULAR_FILE, mode);
  Tree tree(std::move(entries));

  // Verify existent path.
  PathComponentPiece existentPath("a_file");
  auto entry = tree.getEntryPtr(existentPath);
  EXPECT_NE(nullptr, entry);
  EXPECT_EQ("a_file", entry->getName());
  EXPECT_EQ(TreeEntryType::BLOB, entry->getType());
  EXPECT_EQ(FileType::REGULAR_FILE, entry->getFileType());
  EXPECT_EQ(mode, entry->getOwnerPermissions());

  // Verify non-existent path.
  PathComponentPiece nonExistentPath("not_a_file");
  EXPECT_EQ(nullptr, tree.getEntryPtr(nonExistentPath));
}
