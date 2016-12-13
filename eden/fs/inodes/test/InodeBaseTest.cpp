/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <gtest/gtest.h>
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using folly::StringPiece;
using std::dynamic_pointer_cast;

TEST(InodeBase, getPath) {
  TestMountBuilder builder;
  builder.addFiles({
      {"a/b/c/noop.c", "int main() { return 0; }\n"},
  });
  auto testMount = builder.build();

  auto root = testMount->getEdenMount()->getRootInode();
  EXPECT_EQ(RelativePathPiece(), root->getPath().value());
  EXPECT_EQ("<root>", root->getLogPath());

  auto getChild = [](
      const std::shared_ptr<TreeInode>& parent, StringPiece name) {
    return parent->getChildByName(PathComponentPiece{name}).get();
  };
  auto childTree = [&getChild](
      const std::shared_ptr<TreeInode>& parent, StringPiece name) {
    return dynamic_pointer_cast<TreeInode>(getChild(parent, name));
  };
  auto childFile = [&getChild](
      const std::shared_ptr<TreeInode>& parent, StringPiece name) {
    return dynamic_pointer_cast<FileInode>(getChild(parent, name));
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
