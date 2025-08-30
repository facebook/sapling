/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/Traverse.h"

#include <gtest/gtest.h>
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;

struct TestCallbacks : TraversalCallbacks {
  std::vector<RelativePath> paths;

  void visitTreeInode(
      RelativePathPiece path,
      InodeNumber ino,
      const std::optional<ObjectId>& id,
      uint64_t fuseRefcount,
      const std::vector<ChildEntry>& entries) override {
    paths.emplace_back(path);
    (void)ino;
    (void)id;
    (void)fuseRefcount;
    (void)entries;
  }

  bool shouldRecurse(const ChildEntry& entry) override {
    (void)entry;
    return true;
  }
};

TEST(TraverseTest, does_not_traverse_unallocated_and_unmaterialized_trees) {
  FakeTreeBuilder builder;
  builder.setFile("dir1/dir2/file", "test\n");
  TestMount mount{builder};

  auto rootPath = RelativePath{""};
  auto root = mount.getTreeInode(rootPath);

  TestCallbacks callbacks;
  traverseObservedInodes(*root, rootPath, callbacks);

  EXPECT_EQ(2, callbacks.paths.size());
  EXPECT_EQ("", callbacks.paths.at(0));
  EXPECT_EQ(".eden", callbacks.paths.at(1));
}

TEST(TraverseTest, does_traverse_loaded_trees) {
  FakeTreeBuilder builder;
  builder.setFile("dir1/dir2/file", "test\n");
  TestMount mount{builder};

  auto rootPath = RelativePath{""};
  auto root = mount.getTreeInode(rootPath);

  // Trigger allocation of dir2 and file.
  auto file = mount.getFileInode("dir1/dir2/file");

  TestCallbacks callbacks;
  traverseObservedInodes(*root, rootPath, callbacks);

  EXPECT_EQ(4, callbacks.paths.size());
  EXPECT_EQ("", callbacks.paths.at(0));
  EXPECT_EQ(".eden", callbacks.paths.at(1));
  EXPECT_EQ("dir1", callbacks.paths.at(2));
  EXPECT_EQ("dir1/dir2", callbacks.paths.at(3));
}
