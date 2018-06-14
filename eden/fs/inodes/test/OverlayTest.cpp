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

#include <folly/FileUtil.h>
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

using namespace folly::string_piece_literals;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using std::string;

namespace facebook {
namespace eden {

TEST(OverlayGoldMasterTest, can_load_overlay_v2) {
  Overlay overlay{realpath("eden/test-data/overlay-v2")};

  InodeTimestamps timestamps;

  Hash hash1{folly::ByteRange{"abcdabcdabcdabcdabcd"_sp}};
  Hash hash2{folly::ByteRange{"01234012340123401234"_sp}};
  Hash hash3{folly::ByteRange{"e0e0e0e0e0e0e0e0e0e0"_sp}};
  Hash hash4{folly::ByteRange{"44444444444444444444"_sp}};

  auto rootTree = overlay.loadOverlayDir(kRootNodeId);
  auto file =
      overlay.openFile(2_ino, Overlay::kHeaderIdentifierFile, timestamps);
  auto subdir = overlay.loadOverlayDir(3_ino);
  auto emptyDir = overlay.loadOverlayDir(4_ino);
  auto hello =
      overlay.openFile(5_ino, Overlay::kHeaderIdentifierFile, timestamps);

  ASSERT_TRUE(rootTree);
  EXPECT_EQ(2, rootTree->first.size());
  const auto& fileEntry = rootTree->first.at("file"_pc);
  EXPECT_EQ(2_ino, fileEntry.getInodeNumber());
  EXPECT_EQ(hash1, fileEntry.getHash());
  EXPECT_EQ(S_IFREG | 0644, fileEntry.getInitialMode());
  const auto& subdirEntry = rootTree->first.at("subdir"_pc);
  EXPECT_EQ(3_ino, subdirEntry.getInodeNumber());
  EXPECT_EQ(hash2, subdirEntry.getHash());
  EXPECT_EQ(S_IFDIR | 0755, subdirEntry.getInitialMode());

  folly::checkUnixError(lseek(file.fd(), Overlay::kHeaderLength, SEEK_SET));
  std::string result;
  folly::readFile(file.fd(), result);
  EXPECT_EQ("contents", result);

  ASSERT_TRUE(subdir);
  EXPECT_EQ(2, subdir->first.size());
  const auto& emptyEntry = subdir->first.at("empty"_pc);
  EXPECT_EQ(4_ino, emptyEntry.getInodeNumber());
  EXPECT_EQ(hash3, emptyEntry.getHash());
  EXPECT_EQ(S_IFDIR | 0755, emptyEntry.getInitialMode());
  const auto& helloEntry = subdir->first.at("hello"_pc);
  EXPECT_EQ(5_ino, helloEntry.getInodeNumber());
  EXPECT_EQ(hash4, helloEntry.getHash());
  EXPECT_EQ(S_IFREG | 0644, helloEntry.getInitialMode());

  ASSERT_TRUE(emptyDir);
  EXPECT_EQ(0, emptyDir->first.size());

  folly::checkUnixError(lseek(hello.fd(), Overlay::kHeaderLength, SEEK_SET));
  folly::readFile(file.fd(), result);
  EXPECT_EQ("", result);
}

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
    beforeRemountFile = inodeFile->getMetadata().timestamps;
  }

  {
    // Check for materialized files.
    mount_.remount();
    auto inodeRemount = mount_.getFileInode("dir/a.txt");
    auto afterRemount = inodeRemount->getMetadata().timestamps;
    expectTimeStampsEqual(beforeRemountFile, afterRemount);
  }

  {
    auto inodeDir = mount_.getTreeInode("dir");
    beforeRemountDir = inodeDir->getMetadata().timestamps;
  }

  {
    // Check for materialized directory
    mount_.remount();
    auto inodeRemount = mount_.getTreeInode("dir");
    auto afterRemount = inodeRemount->getMetadata().timestamps;
    expectTimeStampsEqual(beforeRemountDir, afterRemount);
  }
}

TEST_F(OverlayTest, roundTripThroughSaveAndLoad) {
  auto hash = Hash{"0123456789012345678901234567890123456789"};

  auto overlay = mount_.getEdenMount()->getOverlay();

  auto ino1 = overlay->allocateInodeNumber();
  auto ino2 = overlay->allocateInodeNumber();
  auto ino3 = overlay->allocateInodeNumber();

  DirContents dir;
  dir.emplace("one"_pc, S_IFREG | 0644, ino2, hash);
  dir.emplace("two"_pc, S_IFDIR | 0755, ino3);

  overlay->saveOverlayDir(ino1, dir, InodeTimestamps{});

  auto result = overlay->loadOverlayDir(ino1);
  ASSERT_TRUE(result);
  const auto* newDir = &result->first;

  EXPECT_EQ(2, newDir->size());
  const auto& one = newDir->find("one"_pc)->second;
  const auto& two = newDir->find("two"_pc)->second;
  EXPECT_EQ(ino2, one.getInodeNumber());
  EXPECT_FALSE(one.isMaterialized());
  EXPECT_EQ(ino3, two.getInodeNumber());
  EXPECT_TRUE(two.isMaterialized());
}

TEST_F(OverlayTest, getFilePath) {
  Overlay::InodePath path;

  path = Overlay::getFilePath(1_ino);
  EXPECT_STREQ("01/1", path.data());
  path = Overlay::getFilePath(1234_ino);
  EXPECT_STREQ("d2/1234", path.data());

  // It's slightly unfortunate that we use hexadecimal for the subdirectory
  // name and decimal for the final inode path.  That doesn't seem worth fixing
  // for now.
  path = Overlay::getFilePath(15_ino);
  EXPECT_STREQ("0f/15", path.data());
  path = Overlay::getFilePath(16_ino);
  EXPECT_STREQ("10/16", path.data());
}

enum class OverlayRestartMode {
  CLEAN,
  UNCLEAN,
};

class RawOverlayTest : public ::testing::TestWithParam<OverlayRestartMode> {
 public:
  RawOverlayTest()
      : testDir_{"eden_raw_overlay_test_"},
        overlay{std::make_unique<Overlay>(
            AbsolutePathPiece{testDir_.path().string()})} {}

  void recreate(folly::Optional<OverlayRestartMode> restartMode = folly::none) {
    overlay->close();
    overlay.reset();
    switch (restartMode.value_or(GetParam())) {
      case OverlayRestartMode::CLEAN:
        break;
      case OverlayRestartMode::UNCLEAN:
        if (unlink((testDir_.path() / "next-inode-number").c_str())) {
          folly::throwSystemError("removing saved inode numebr");
        }
        break;
    }
    overlay.reset(new Overlay{AbsolutePathPiece{testDir_.path().string()}});
  }

  folly::test::TemporaryDirectory testDir_;
  std::unique_ptr<Overlay> overlay;
};

TEST_P(RawOverlayTest, max_inode_number_is_1_if_overlay_is_empty) {
  EXPECT_EQ(kRootNodeId, overlay->scanForNextInodeNumber());
  EXPECT_EQ(2_ino, overlay->allocateInodeNumber());

  recreate(OverlayRestartMode::CLEAN);

  EXPECT_EQ(2_ino, overlay->scanForNextInodeNumber());
  EXPECT_EQ(3_ino, overlay->allocateInodeNumber());

  recreate(OverlayRestartMode::UNCLEAN);

  EXPECT_EQ(kRootNodeId, overlay->scanForNextInodeNumber());
  EXPECT_EQ(2_ino, overlay->allocateInodeNumber());
}

TEST_P(RawOverlayTest, remembers_max_inode_number_of_tree_inodes) {
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);

  DirContents dir;
  overlay->saveOverlayDir(ino2, dir, InodeTimestamps{});

  recreate();

  EXPECT_EQ(2_ino, overlay->scanForNextInodeNumber());
}

TEST_P(RawOverlayTest, remembers_max_inode_number_of_tree_entries) {
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);
  auto ino3 = overlay->allocateInodeNumber();
  auto ino4 = overlay->allocateInodeNumber();

  DirContents dir;
  dir.emplace(PathComponentPiece{"f"}, S_IFREG | 0644, ino3);
  dir.emplace(PathComponentPiece{"d"}, S_IFDIR | 0755, ino4);
  overlay->saveOverlayDir(kRootNodeId, dir, InodeTimestamps{});

  recreate();

  EXPECT_EQ(4_ino, overlay->scanForNextInodeNumber());
}

TEST_P(RawOverlayTest, remembers_max_inode_number_of_file) {
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);
  auto ino3 = overlay->allocateInodeNumber();

  // When materializing, overlay data is written leaf-to-root.

  // The File is written first.
  overlay->createOverlayFile(
      ino3, InodeTimestamps{}, folly::ByteRange{"contents"_sp});

  recreate();

  EXPECT_EQ(3_ino, overlay->scanForNextInodeNumber());
}

TEST_P(RawOverlayTest, inode_numbers_not_reused_after_unclean_shutdown) {
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);
  overlay->allocateInodeNumber();
  auto ino4 = overlay->allocateInodeNumber();
  auto ino5 = overlay->allocateInodeNumber();

  // When materializing, overlay data is written leaf-to-root.

  // The File is written first.
  overlay->createOverlayFile(
      ino5, InodeTimestamps{}, folly::ByteRange{"contents"_sp});

  // The subdir is written next.
  DirContents subdir;
  subdir.emplace(PathComponentPiece{"f"}, S_IFREG | 0644, ino5);
  overlay->saveOverlayDir(ino4, subdir, InodeTimestamps{});

  // Crashed before root was written.

  recreate();

  EXPECT_EQ(5_ino, overlay->scanForNextInodeNumber());
}

TEST_P(RawOverlayTest, inode_numbers_after_takeover) {
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);
  auto ino3 = overlay->allocateInodeNumber();
  auto ino4 = overlay->allocateInodeNumber();
  auto ino5 = overlay->allocateInodeNumber();

  // Write a subdir.
  DirContents subdir;
  subdir.emplace(PathComponentPiece{"f"}, S_IFREG | 0644, ino5);
  overlay->saveOverlayDir(ino4, subdir, InodeTimestamps{});

  // Write the root.
  DirContents dir;
  dir.emplace(PathComponentPiece{"f"}, S_IFREG | 0644, ino3);
  dir.emplace(PathComponentPiece{"d"}, S_IFDIR | 0755, ino4);
  overlay->saveOverlayDir(kRootNodeId, dir, InodeTimestamps{});

  recreate();

  overlay->scanForNextInodeNumber();

  // Rewrite the root (say, after a takeover) without the file.

  DirContents newroot;
  newroot.emplace(PathComponentPiece{"d"}, S_IFDIR | 0755, 4_ino);
  overlay->saveOverlayDir(kRootNodeId, newroot, InodeTimestamps{});

  recreate(OverlayRestartMode::CLEAN);

  // Ensure an inode in the overlay but not referenced by the previous session
  // counts.
  EXPECT_EQ(5_ino, overlay->scanForNextInodeNumber());
}

INSTANTIATE_TEST_CASE_P(
    Clean,
    RawOverlayTest,
    ::testing::Values(OverlayRestartMode::CLEAN));

INSTANTIATE_TEST_CASE_P(
    Unclean,
    RawOverlayTest,
    ::testing::Values(OverlayRestartMode::UNCLEAN));

} // namespace eden
} // namespace facebook
