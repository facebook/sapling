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

using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using std::string;

namespace facebook {
namespace eden {

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
      const EdenTimestamp& at,
      const EdenTimestamp& bt) {
    auto a = at.toTimespec();
    auto b = bt.toTimespec();
    EXPECT_EQ(a.tv_sec, b.tv_sec);
    EXPECT_EQ(a.tv_nsec, b.tv_nsec);
  }

  static void expectTimeStampsEqual(
      const InodeTimestamps& a,
      const InodeTimestamps& b) {
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
  InodeTimestamps beforeRemountFile;
  InodeTimestamps beforeRemountDir;
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
    auto afterRemount = inodeRemount->getTimestamps();
    expectTimeStampsEqual(beforeRemountDir, afterRemount);
  }
}

TEST_F(OverlayTest, roundTripThroughSaveAndLoad) {
  auto hash = Hash{"0123456789012345678901234567890123456789"};

  auto overlay = mount_.getEdenMount()->getOverlay();

  TreeInode::Dir dir;
  dir.entries.emplace("one"_pc, S_IFREG | 0644, 11_ino, hash);
  dir.entries.emplace("two"_pc, S_IFDIR | 0755, 12_ino);

  overlay->saveOverlayDir(10_ino, dir);

  auto newDir =
      overlay->loadOverlayDir(10_ino, mount_.getEdenMount()->getInodeMap());
  EXPECT_TRUE(newDir);

  EXPECT_EQ(2, newDir->entries.size());
  const auto& one = newDir->entries.find("one"_pc)->second;
  const auto& two = newDir->entries.find("two"_pc)->second;
  EXPECT_EQ(11_ino, one.getInodeNumber());
  EXPECT_FALSE(one.isMaterialized());
  EXPECT_EQ(12_ino, two.getInodeNumber());
  EXPECT_TRUE(two.isMaterialized());
}

TEST_F(OverlayTest, getFilePath) {
  auto overlay = mount_.getEdenMount()->getOverlay();
  std::array<char, Overlay::kMaxPathLength> path;

  Overlay::getFilePath(1_ino, path);
  EXPECT_STREQ("01/1", path.data());
  Overlay::getFilePath(1234_ino, path);
  EXPECT_STREQ("d2/1234", path.data());

  // It's slightly unfortunate that we use hexadecimal for the subdirectory
  // name and decimal for the final inode path.  That doesn't seem worth fixing
  // for now.
  Overlay::getFilePath(15_ino, path);
  EXPECT_STREQ("0f/15", path.data());
  Overlay::getFilePath(16_ino, path);
  EXPECT_STREQ("10/16", path.data());
}

} // namespace eden
} // namespace facebook
