/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/InodeBase.h"
#include <gtest/gtest.h>
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using folly::StringPiece;
using std::dynamic_pointer_cast;

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
    return parent->getChildByName(PathComponentPiece{name}).get();
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
