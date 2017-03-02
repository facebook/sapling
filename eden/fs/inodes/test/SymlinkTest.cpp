/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/InodeMap.h"

#include <folly/Bits.h>
#include <folly/Format.h>
#include <folly/String.h>
#include <gtest/gtest.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/utils/Bug.h"
#include "eden/utils/test/TestChecks.h"

using namespace facebook::eden;
using folly::StringPiece;

class SymlinkTest : public ::testing::Test {
 protected:
  void SetUp() override {
    // Set up a directory structure that we will use for most
    // of the tests below
    TestMountBuilder builder;
    builder.addFiles({
        {"doc.txt", "hello\n"},
    });
    mount_ = builder.build();
    mount_->mkdir("a");
  }

  std::unique_ptr<TestMount> mount_;
};

TEST_F(SymlinkTest, makeSymlink) {
  StringPiece name{"s1"}; // node to create in the filesystem
  StringPiece target{"foo!"}; // the value we want readlink to return

  auto root = mount_->getTreeInode(RelativePathPiece());
  auto inode = root->symlink(PathComponentPiece{name}, target);
  EXPECT_EQ(inode->readlink().get(), target);
  // Let's make sure that we can load it up and see the right results
  auto loadedInode = mount_->getFileInode(RelativePathPiece{name});
  EXPECT_EQ(loadedInode->readlink().get(), target);
}

TEST_F(SymlinkTest, makeSymlinkCollisionFile) {
  StringPiece name{"doc.txt"}; // node to create in the filesystem
  StringPiece target{"foo!"}; // the value we want readlink to return

  auto root = mount_->getTreeInode(RelativePathPiece());
  // Name already exists, so we expect this to fail
  EXPECT_THROW_ERRNO(root->symlink(PathComponentPiece{name}, target), EEXIST);
}

TEST_F(SymlinkTest, makeSymlinkCollisionDir) {
  StringPiece name{"a"}; // node to create in the filesystem
  StringPiece target{"foo!"}; // the value we want readlink to return

  auto root = mount_->getTreeInode(RelativePathPiece());
  // Name already exists, so we expect this to fail
  EXPECT_THROW_ERRNO(root->symlink(PathComponentPiece{name}, target), EEXIST);
}
