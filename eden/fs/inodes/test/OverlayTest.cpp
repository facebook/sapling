/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/Overlay.h"

#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/service/PrettyPrinters.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using folly::Future;
using folly::StringPiece;
using folly::makeFuture;
using std::string;

class OverlayTest : public ::testing::Test {
 protected:
  void SetUp() override {
    // Set up a directory structure that we will use for most
    // of the tests below
    FakeTreeBuilder builder;
    builder.setFiles({
        {"dir/a.txt", "This is a.txt.\n"},
    });
    mount_.initialize(builder);
  }

  // Helper method to check if two timestamps are same or not.
  static void expectTimeSpecsEqual(
      const struct timespec& a,
      const struct timespec& b) {
    EXPECT_EQ(a.tv_sec, b.tv_sec);
    EXPECT_EQ(a.tv_nsec, b.tv_nsec);
  }

  static void expectTimeStampsEqual(
      const InodeBase::InodeTimestamps& a,
      const InodeBase::InodeTimestamps& b) {
    expectTimeSpecsEqual(a.atime, b.atime);
    expectTimeSpecsEqual(a.mtime, b.mtime);
    expectTimeSpecsEqual(a.ctime, b.ctime);
  }

  TestMount mount_;
};

TEST_F(OverlayTest, testRemount) {
  mount_.addFile("dir/new.txt", "test\n");
  mount_.remount();
  // Confirm that the tree has been updated correctly.
  auto newInode = mount_.getFileInode("dir/new.txt");
  EXPECT_FILE_INODE(newInode, "test\n", 0644);
}

TEST_F(OverlayTest, testModifyRemount) {
  // inode object has to be destroyed
  // before remount is called to release the reference
  {
    auto inode = mount_.getFileInode("dir/a.txt");
    EXPECT_FILE_INODE(inode, "This is a.txt.\n", 0644);
  }

  // materialize a directory
  mount_.overwriteFile("dir/a.txt", "contents changed\n");
  mount_.remount();

  auto newInode = mount_.getFileInode("dir/a.txt");
  EXPECT_FILE_INODE(newInode, "contents changed\n", 0644);
}

// In memory timestamps should be same before and after a remount.
// (inmemory timestamps should be written to overlay on
// on unmount and should be read back from the overlay on remount)
TEST_F(OverlayTest, testTimeStampsInOverlayOnMountAndUnmount) {
  // Materialize file and directory
  // test timestamp behavior in overlay on remount.
  InodeBase::InodeTimestamps beforeRemountFile;
  InodeBase::InodeTimestamps beforeRemountDir;
  mount_.overwriteFile("dir/a.txt", "contents changed\n");

  {
    // We do not want to keep references to inode in order to remount.
    auto inodeFile = mount_.getFileInode("dir/a.txt");
    EXPECT_FILE_INODE(inodeFile, "contents changed\n", 0644);
    beforeRemountFile = inodeFile->getTimestamps();
  }

  {
    // Check for materialized files.
    mount_.remount();
    auto inodeRemount = mount_.getFileInode("dir/a.txt");
    auto afterRemount = inodeRemount->getTimestamps();
    expectTimeStampsEqual(beforeRemountFile, afterRemount);
  }

  {
    auto inodeDir = mount_.getTreeInode("dir");
    beforeRemountDir = inodeDir->getTimestamps();
  }

  {
    // Check for materialized directory
    mount_.remount();
    auto inodeRemount = mount_.getTreeInode("dir");
    InodeBase::InodeTimestamps afterRemount = inodeRemount->getTimestamps();
    expectTimeStampsEqual(beforeRemountDir, afterRemount);
  }
}
