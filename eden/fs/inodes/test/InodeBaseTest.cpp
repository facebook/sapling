/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeBase.h"
#include <folly/portability/GTest.h>
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using folly::StringPiece;

TEST(InodeBase, getPath) {
  FakeTreeBuilder builder;
  builder.setFiles({
      {"a/b/c/noop.c", "int main() { return 0; }\n"},
  });
  TestMount testMount{builder};

  auto root = testMount.getEdenMount()->getRootInode();
  EXPECT_EQ(RelativePathPiece(), root->getPath().value());
  EXPECT_EQ("<root>", root->getLogPath());

  auto getChild = [](const TreeInodePtr& parent, StringPiece name) {
    return parent
        ->getOrLoadChild(
            PathComponentPiece{name}, ObjectFetchContext::getNullContext())
        .get();
  };
  auto childTree = [&getChild](const TreeInodePtr& parent, StringPiece name) {
    return getChild(parent, name).asTreePtr();
  };
  auto childFile = [&getChild](const TreeInodePtr& parent, StringPiece name) {
    return getChild(parent, name).asFilePtr();
  };

  auto a = childTree(root, "a");
  EXPECT_EQ(RelativePath{"a"}, a->getPath().value());
  EXPECT_EQ("a", a->getLogPath());

  auto ab = childTree(a, "b");
  EXPECT_EQ(RelativePath{"a/b"}, ab->getPath().value());
  EXPECT_EQ("a/b", ab->getLogPath());

  auto abc = childTree(ab, "c");
  EXPECT_EQ(RelativePath{"a/b/c"}, abc->getPath().value());
  EXPECT_EQ("a/b/c", abc->getLogPath());

  auto noopC = childFile(abc, "noop.c");
  EXPECT_EQ(RelativePath{"a/b/c/noop.c"}, noopC->getPath().value());
  EXPECT_EQ("a/b/c/noop.c", noopC->getLogPath());

  // TODO: Test that the path gets updated after unlink() and rename()
  // operations.
  //
  // Currently calling TreeInode::unlink() and TreeInode::rename() here does
  // not work.  (TreeInode::getChildByName() does not correctly register new
  // inodes it creates in the EdenDispatcher's inode map.  The unlink() and
  // rename() operations require that the inode exist in the dispatcher map.)
  //
  // I am currently working on refactoring the inode map in a subsequent diff.
  // My refactoring ensures that inodes always get registered correctly,
  // regardless of how they are created.  I'll come back and work on test cases
  // here once my refactored InodeMap code lands.
}

class InodeBaseEnsureMaterializedTest : public ::testing::Test {
 protected:
  void SetUp() override {
    FakeTreeBuilder builder;
    builder.setFiles(
        {{"dir/a.txt", "This is a.txt.\n"},
         {"dir2/a2.txt", "This is a2.txt.\n"},
         {"dir/sub/b.txt", "This is b.txt.\n"},
         {"dir/sub/sub2/c.txt", "This is c.txt.\n"}});
    mount_.initialize(builder);
  }

  TestMount mount_;
};

#ifndef _WIN32
namespace {
bool isInodeMaterialized(const TreeInodePtr& inode) {
  return inode->getContents().wlock()->isMaterialized();
}

bool isInodeMaterialized(const FileInodePtr& inode) {
  return !inode->getBlobHash().has_value();
}

} // namespace

TEST_F(InodeBaseEnsureMaterializedTest, testFile) {
  auto regularFile = mount_.getFileInode("dir/a.txt");
  EXPECT_FALSE(isInodeMaterialized(regularFile));
  (void)regularFile
      ->ensureMaterialized(ObjectFetchContext::getNullContext(), false)
      .get();

  EXPECT_TRUE(isInodeMaterialized(regularFile));
  // The parent tree should also be materialized
  auto parentTree = mount_.getTreeInode("dir");
  EXPECT_TRUE(isInodeMaterialized(parentTree));
}

TEST_F(InodeBaseEnsureMaterializedTest, testFileAlreadyMaterialized) {
  auto regularFile = mount_.getFileInode("dir/a.txt");
  EXPECT_FALSE(isInodeMaterialized(regularFile));
  (void)regularFile
      ->ensureMaterialized(ObjectFetchContext::getNullContext(), false)
      .get();
  // The parent tree should also be materialized
  auto parentTree = mount_.getTreeInode("dir");
  EXPECT_TRUE(isInodeMaterialized(parentTree));

  // Should be fine if we call ensureMaterialized if a file is materialized
  (void)regularFile
      ->ensureMaterialized(ObjectFetchContext::getNullContext(), false)
      .get();
  EXPECT_TRUE(isInodeMaterialized(regularFile));
}

TEST_F(InodeBaseEnsureMaterializedTest, testSymlinksNoFollow) {
  StringPiece name{"s1"};
  StringPiece target{"sub/b.txt"};
  auto tree = mount_.getTreeInode("dir");
  auto inode =
      tree->symlink(PathComponentPiece{name}, target, InvalidationRequired::No);
  (void)inode->ensureMaterialized(ObjectFetchContext::getNullContext(), false)
      .get();

  auto fileB = mount_.getFileInode("dir/sub/b.txt");
  EXPECT_FALSE(isInodeMaterialized(fileB));
}

TEST_F(InodeBaseEnsureMaterializedTest, testSymlinksFollow) {
  StringPiece name{"s1"};
  StringPiece target{"sub/b.txt"};
  auto tree = mount_.getTreeInode("dir");
  auto inode =
      tree->symlink(PathComponentPiece{name}, target, InvalidationRequired::No);
  (void)inode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get();

  auto fileB = mount_.getFileInode("dir/sub/b.txt");
  EXPECT_TRUE(isInodeMaterialized(fileB));
}

TEST_F(InodeBaseEnsureMaterializedTest, testSymlinksOutOfMountNoThrow) {
  StringPiece name{"s1"};
  // This path is out of mount, EnsureMaterialized does not supported but should
  // be a soft error
  StringPiece target{"../../../out_dir/b.txt"};
  auto tree = mount_.getTreeInode("dir");
  auto inode =
      tree->symlink(PathComponentPiece{name}, target, InvalidationRequired::No);
  (void)inode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get();
}

TEST_F(InodeBaseEnsureMaterializedTest, testSymlinksAbsolutePathNoThrow) {
  StringPiece name{"s1"};
  // This path is an absolute path, EnsureMaterialized does not supported but
  // should be a soft error
  StringPiece target{"/home/out_dir/b.txt"};
  auto tree = mount_.getTreeInode("dir");
  auto inode =
      tree->symlink(PathComponentPiece{name}, target, InvalidationRequired::No);
  (void)inode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get();
}

TEST_F(InodeBaseEnsureMaterializedTest, testSymlinksNonUtf8Exception) {
  StringPiece name{"s1"};
  // None UTF path is not supported and should throw an exception.
  StringPiece target{"sub/a\xe0\xa0\x80z\xa0\u8138\u4e66\t\u03c0"};
  auto tree = mount_.getTreeInode("dir");
  auto inode =
      tree->symlink(PathComponentPiece{name}, target, InvalidationRequired::No);
  EXPECT_THROW(
      inode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
          .get(),
      facebook::eden::PathComponentNotUtf8);
}

TEST_F(InodeBaseEnsureMaterializedTest, testTree) {
  // EnsureMaterialized a tree should materialize everything under the tree
  // recuresivley.
  auto tree = mount_.getTreeInode("dir");
  EXPECT_FALSE(isInodeMaterialized(tree));

  StringPiece name{"s1"};
  StringPiece target{"../dir2/a2.txt"};
  // Symlink dir/s1 links to dir2/a2.txt
  auto symlink =
      tree->symlink(PathComponentPiece{name}, target, InvalidationRequired::No);

  (void)tree->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get();

  EXPECT_TRUE(isInodeMaterialized(tree));

  auto fileA = mount_.getFileInode("dir/a.txt");
  EXPECT_TRUE(isInodeMaterialized(fileA));

  auto subTree = mount_.getTreeInode("dir/sub");
  EXPECT_TRUE(isInodeMaterialized(subTree));

  auto fileB = mount_.getFileInode("dir/sub/b.txt");
  EXPECT_TRUE(isInodeMaterialized(fileB));

  auto subTree2 = mount_.getTreeInode("dir/sub/sub2");
  EXPECT_TRUE(isInodeMaterialized(subTree2));

  auto fileC = mount_.getFileInode("dir/sub/sub2/c.txt");
  EXPECT_TRUE(isInodeMaterialized(fileC));

  // dir2/a2.txt should be materialized as dir/s1 is requested to be
  // materialized and follows symlink
  auto fileA2 = mount_.getFileInode("dir2/a2.txt");
  EXPECT_TRUE(isInodeMaterialized(fileA2));

  auto tree2 = mount_.getTreeInode("dir2");
  EXPECT_TRUE(isInodeMaterialized(tree2));
}

TEST_F(InodeBaseEnsureMaterializedTest, testSymlinkTree) {
  auto tree2 = mount_.getTreeInode("dir2");
  EXPECT_FALSE(isInodeMaterialized(tree2));

  auto tree = mount_.getTreeInode("dir");
  EXPECT_FALSE(isInodeMaterialized(tree));

  StringPiece name{"s1"};
  StringPiece target{"../dir"};
  // Symlink dir2/s1 links to dir
  auto symlink = tree2->symlink(
      PathComponentPiece{name}, target, InvalidationRequired::No);

  // So ensureMaterialize symlink dir2/s1 should materialize dir and its
  // children recursively
  (void)symlink->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get();

  EXPECT_TRUE(isInodeMaterialized(tree));

  auto fileA = mount_.getFileInode("dir/a.txt");
  EXPECT_TRUE(isInodeMaterialized(fileA));

  auto subTree = mount_.getTreeInode("dir/sub");
  EXPECT_TRUE(isInodeMaterialized(subTree));

  auto fileB = mount_.getFileInode("dir/sub/b.txt");
  EXPECT_TRUE(isInodeMaterialized(fileB));

  auto subTree2 = mount_.getTreeInode("dir/sub/sub2");
  EXPECT_TRUE(isInodeMaterialized(subTree2));

  auto fileC = mount_.getFileInode("dir/sub/sub2/c.txt");
  EXPECT_TRUE(isInodeMaterialized(fileC));
}
#endif
