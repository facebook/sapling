/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/TreeInode.h"

#include <gtest/gtest.h>
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"

using namespace facebook::eden;

static DirEntry makeDirEntry() {
  return DirEntry{S_IFREG | 0644, 1_ino, Hash{}};
}

static TreeEntry makeTreeEntry(folly::StringPiece name) {
  return TreeEntry{Hash{}, name, TreeEntryType::REGULAR_FILE};
}

TEST(TreeInode, findEntryDifferencesWithSameEntriesReturnsNone) {
  DirContents dir;
  dir.entries.emplace("one"_pc, makeDirEntry());
  dir.entries.emplace("two"_pc, makeDirEntry());
  Tree tree{{makeTreeEntry("one"), makeTreeEntry("two")}};

  EXPECT_FALSE(findEntryDifferences(dir, tree));
}

TEST(TreeInode, findEntryDifferencesReturnsAdditionsAndSubtractions) {
  DirContents dir;
  dir.entries.emplace("one"_pc, makeDirEntry());
  dir.entries.emplace("two"_pc, makeDirEntry());
  Tree tree{{makeTreeEntry("one"), makeTreeEntry("three")}};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"+ three", "- two"}), *differences);
}

TEST(TreeInode, findEntryDifferencesWithOneSubtraction) {
  DirContents dir;
  dir.entries.emplace("one"_pc, makeDirEntry());
  dir.entries.emplace("two"_pc, makeDirEntry());
  Tree tree{{makeTreeEntry("one")}};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"- two"}), *differences);
}

TEST(TreeInode, findEntryDifferencesWithOneAddition) {
  DirContents dir;
  dir.entries.emplace("one"_pc, makeDirEntry());
  dir.entries.emplace("two"_pc, makeDirEntry());
  Tree tree{
      {makeTreeEntry("one"), makeTreeEntry("two"), makeTreeEntry("three")}};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"+ three"}), *differences);
}
