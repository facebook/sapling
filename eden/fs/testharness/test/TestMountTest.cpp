/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/Range.h>
#include "eden/fs/inodes/TreeEntryFileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/testharness/TestMount.h"

#include <gtest/gtest.h>

using namespace facebook::eden;
using folly::ByteRange;
using folly::StringPiece;

TEST(TestMount, createEmptyMount) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto rootTree = testMount->getRootTree();
  EXPECT_EQ(0, rootTree->getTreeEntries().size())
      << "Initially, the tree should be empty.";
}

TEST(TestMount, createSimpleTestMount) {
  TestMountBuilder builder;
  builder.addFile({"path1", "first!"});
  builder.addFiles({
      // clang-format off
      {"path2", "hello"},
      {"path3", "world"},
      // clang-format on
  });
  auto testMount = builder.build();

  auto fileTreeEntry = testMount->getFileInodeForPath("path1");
  EXPECT_NE(nullptr, fileTreeEntry.get())
      << "Should be able to find TreeEntryFileInode for path1";

  auto entry = fileTreeEntry->getEntry();
  auto expectedSha1 = Hash::sha1(ByteRange(StringPiece("first!")));
  EXPECT_EQ(expectedSha1, entry->hash.value())
      << "For simplicity, TestMount uses the SHA-1 of the contents as "
      << "the id for a Blob.";

  auto dirTreeEntry = testMount->getDirInodeForPath("");
  {
    auto dir = dirTreeEntry->getContents().rlock();
    auto& rootEntries = dir->entries;
    auto& path1Entry = rootEntries.at(PathComponentPiece("path1"));
    EXPECT_EQ(expectedSha1, path1Entry->hash.value())
        << "Getting the Entry from the root Dir should also work.";
  }

  auto rootTree = testMount->getRootTree();
  EXPECT_EQ(3, rootTree->getTreeEntries().size())
      << "Root Tree object should have 3 entries: path1, path2, path3";
}

TEST(TestMount, addFileAfterMountIsCreated) {
  TestMountBuilder builder;
  builder.addFile(
      {"file1.txt", "I am in the original commit that is backing the mount."});
  auto testMount = builder.build();

  testMount->addFile("file2.txt", "I am added by the user after mounting.");
  auto dirTreeEntry = testMount->getDirInodeForPath("");
  {
    auto dir = dirTreeEntry->getContents().rlock();
    auto& rootEntries = dir->entries;
    EXPECT_EQ(2, rootEntries.size()) << "New entry is visible in MountPoint";
  }

  auto rootTree = testMount->getRootTree();
  EXPECT_EQ(1, rootTree->getTreeEntries().size())
      << "New entry is not in the Tree, though.";
}
