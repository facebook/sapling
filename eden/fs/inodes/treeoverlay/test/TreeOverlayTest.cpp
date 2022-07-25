/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/TreeOverlay.h"
#include "eden/fs/inodes/test/OverlayTestUtil.h"

#include <iomanip>

#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/utils/DirType.h"

namespace facebook::eden {

#ifdef _WIN32
class TreeOverlayTest : public ::testing::TestWithParam<Overlay::OverlayType> {
 protected:
  Overlay::OverlayType overlayType() const {
    return GetParam();
  }

  void SetUp() override {
    // Set up a directory structure that we will use for most
    // of the tests below
    FakeTreeBuilder builder;
    builder.mkdir("dir");
    builder.mkdir("foo");
    builder.mkdir("foo/bar");
    mount_.initialize(builder, overlayType());
  }

  TestMount mount_;
};

TEST_P(TreeOverlayTest, roundTripThroughSaveAndLoad) {
  auto hash = ObjectId::fromHex("0123456789012345678901234567890123456789");

  auto overlay = mount_.getEdenMount()->getOverlay();

  auto ino1 = overlay->allocateInodeNumber();
  auto ino2 = overlay->allocateInodeNumber();
  auto ino3 = overlay->allocateInodeNumber();

  DirContents dir(kPathMapDefaultCaseSensitive);
  dir.emplace("one"_pc, S_IFREG | 0644, ino2, hash);
  dir.emplace("two"_pc, S_IFDIR | 0755, ino3);

  overlay->saveOverlayDir(ino1, dir);

  auto result = overlay->loadOverlayDir(ino1);
  ASSERT_TRUE(!result.empty());

  EXPECT_EQ(2, result.size());
  const auto& one = result.find("one"_pc)->second;
  const auto& two = result.find("two"_pc)->second;
  EXPECT_EQ(ino2, one.getInodeNumber());
  EXPECT_FALSE(one.isMaterialized());
  EXPECT_EQ(ino3, two.getInodeNumber());
  EXPECT_TRUE(two.isMaterialized());
}

INSTANTIATE_TEST_SUITE_P(
    TreeOverlayTest,
    TreeOverlayTest,
    ::testing::Values(
        Overlay::OverlayType::Tree,
        Overlay::OverlayType::TreeBuffered));

#endif

TEST(PlainTreeOverlayTest, new_overlay_is_clean) {
  folly::test::TemporaryDirectory testDir;
  auto overlay = Overlay::create(
      AbsolutePath{testDir.path().string()},
      kPathMapDefaultCaseSensitive,
      Overlay::OverlayType::Tree,
      std::make_shared<NullStructuredLogger>(),
      *EdenConfig::createTestEdenConfig());
  overlay->initialize().get();
  EXPECT_TRUE(overlay->hadCleanStartup());
}

TEST(PlainTreeOverlayTest, new_overlay_is_clean_buffered) {
  folly::test::TemporaryDirectory testDir;
  auto overlay = Overlay::create(
      AbsolutePath{testDir.path().string()},
      kPathMapDefaultCaseSensitive,
      Overlay::OverlayType::TreeBuffered,
      std::make_shared<NullStructuredLogger>(),
      *EdenConfig::createTestEdenConfig());
  overlay->initialize().get();
  EXPECT_TRUE(overlay->hadCleanStartup());
}

TEST(PlainTreeOverlayTest, reopened_overlay_is_clean) {
  folly::test::TemporaryDirectory testDir;
  {
    auto overlay = Overlay::create(
        AbsolutePath{testDir.path().string()},
        kPathMapDefaultCaseSensitive,
        Overlay::OverlayType::Tree,
        std::make_shared<NullStructuredLogger>(),
        *EdenConfig::createTestEdenConfig());
    overlay->initialize().get();
  }
  auto overlay = Overlay::create(
      AbsolutePath{testDir.path().string()},
      kPathMapDefaultCaseSensitive,
      Overlay::OverlayType::Tree,
      std::make_shared<NullStructuredLogger>(),
      *EdenConfig::createTestEdenConfig());
  overlay->initialize().get();
  EXPECT_TRUE(overlay->hadCleanStartup());
}

TEST(PlainTreeOverlayTest, reopened_overlay_is_clean_buffered) {
  folly::test::TemporaryDirectory testDir;
  {
    auto overlay = Overlay::create(
        AbsolutePath{testDir.path().string()},
        kPathMapDefaultCaseSensitive,
        Overlay::OverlayType::TreeBuffered,
        std::make_shared<NullStructuredLogger>(),
        *EdenConfig::createTestEdenConfig());
    overlay->initialize().get();
  }
  auto overlay = Overlay::create(
      AbsolutePath{testDir.path().string()},
      kPathMapDefaultCaseSensitive,
      Overlay::OverlayType::TreeBuffered,
      std::make_shared<NullStructuredLogger>(),
      *EdenConfig::createTestEdenConfig());
  overlay->initialize().get();
  EXPECT_TRUE(overlay->hadCleanStartup());
}

TEST(PlainTreeOverlayTest, close_overlay_with_no_capacity_buffered) {
  auto config = EdenConfig::createTestEdenConfig();
  config->overlayBufferSize.setValue(0, ConfigSource::Default, true);
  folly::test::TemporaryDirectory testDir;
  auto overlay = Overlay::create(
      AbsolutePath{testDir.path().string()},
      kPathMapDefaultCaseSensitive,
      Overlay::OverlayType::TreeBuffered,
      std::make_shared<NullStructuredLogger>(),
      *config);
  overlay->initialize().get();
  overlay->close();
  EXPECT_TRUE(overlay->isClosed());
}

class RawTreeOverlayTest
    : public ::testing::TestWithParam<Overlay::OverlayType> {
 public:
  RawTreeOverlayTest() : testDir_{makeTempDir("eden_raw_overlay_test_")} {
    loadOverlay();
  }

  Overlay::OverlayType overlayType() const {
    return GetParam();
  }

  void recreate() {
    unloadOverlay();
    loadOverlay();
  }

  void unloadOverlay() {
    overlay->close();
    overlay = nullptr;
  }

  void loadOverlay() {
    overlay = Overlay::create(
        getLocalDir(),
        kPathMapDefaultCaseSensitive,
        overlayType(),
        std::make_shared<NullStructuredLogger>(),
        *EdenConfig::createTestEdenConfig());
    overlay->initialize().get();
  }

  AbsolutePath getLocalDir() {
    return AbsolutePath{testDir_.path().string()};
  }

  folly::test::TemporaryDirectory testDir_;
  std::shared_ptr<Overlay> overlay;
};

TEST_P(RawTreeOverlayTest, cannot_save_overlay_dir_when_closed) {
  overlay->close();
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);

  DirContents dir(kPathMapDefaultCaseSensitive);
  EXPECT_THROW_RE(
      overlay->saveOverlayDir(ino2, dir),
      std::system_error,
      "cannot access overlay after it is closed");
}

TEST_P(RawTreeOverlayTest, max_inode_number_is_1_if_overlay_is_empty) {
  EXPECT_EQ(kRootNodeId, overlay->getMaxInodeNumber());
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);

  recreate();

  EXPECT_EQ(kRootNodeId, overlay->getMaxInodeNumber());
  ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);

  DirContents dir(kPathMapDefaultCaseSensitive);
  overlay->saveOverlayDir(ino2, dir);

  recreate();

  EXPECT_EQ(kRootNodeId, overlay->getMaxInodeNumber());
}

TEST_P(RawTreeOverlayTest, remembers_max_inode_number_of_tree_entries) {
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);
  auto ino3 = overlay->allocateInodeNumber();
  auto ino4 = overlay->allocateInodeNumber();

  DirContents dir(kPathMapDefaultCaseSensitive);
  dir.emplace(PathComponentPiece{"f"}, S_IFREG | 0644, ino3);
  dir.emplace(PathComponentPiece{"d"}, S_IFDIR | 0755, ino4);
  overlay->saveOverlayDir(kRootNodeId, dir);

  recreate();

  SCOPED_TRACE("Inodes:\n" + debugDumpOverlayInodes(*overlay, kRootNodeId));
  EXPECT_EQ(4_ino, overlay->getMaxInodeNumber());
}

TEST_P(RawTreeOverlayTest, inode_numbers_after_takeover) {
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);
  auto ino3 = overlay->allocateInodeNumber();
  auto ino4 = overlay->allocateInodeNumber();
  auto ino5 = overlay->allocateInodeNumber();

  // Write a subdir.
  DirContents subdir(kPathMapDefaultCaseSensitive);
  subdir.emplace(PathComponentPiece{"f"}, S_IFREG | 0644, ino5);
  overlay->saveOverlayDir(ino4, subdir);

  // Write the root.
  DirContents dir(kPathMapDefaultCaseSensitive);
  dir.emplace(PathComponentPiece{"f"}, S_IFREG | 0644, ino3);
  dir.emplace(PathComponentPiece{"d"}, S_IFDIR | 0755, ino4);
  overlay->saveOverlayDir(kRootNodeId, dir);

  recreate();

  // Rewrite the root (say, after a takeover) without the file.

  DirContents newroot(kPathMapDefaultCaseSensitive);
  newroot.emplace(PathComponentPiece{"d"}, S_IFDIR | 0755, 4_ino);
  overlay->saveOverlayDir(kRootNodeId, newroot);

  recreate();

  SCOPED_TRACE("Inodes:\n" + debugDumpOverlayInodes(*overlay, kRootNodeId));
  // Ensure an inode in the overlay but not referenced by the previous session
  // counts.
  EXPECT_EQ(5_ino, overlay->getMaxInodeNumber());
}

INSTANTIATE_TEST_SUITE_P(
    RawTreeOverlayTest,
    RawTreeOverlayTest,
    ::testing::Values(
        Overlay::OverlayType::Tree,
        Overlay::OverlayType::TreeBuffered));

class DebugDumpTreeOverlayInodesTest
    : public ::testing::TestWithParam<Overlay::OverlayType> {
 public:
  Overlay::OverlayType overlayType() const {
    return GetParam();
  }

  DebugDumpTreeOverlayInodesTest()
      : testDir_{makeTempDir("eden_DebugDumpTreeOverlayInodesTest")} {
    overlay = Overlay::create(
        AbsolutePathPiece{testDir_.path().string()},
        kPathMapDefaultCaseSensitive,
        overlayType(),
        std::make_shared<NullStructuredLogger>(),
        *EdenConfig::createTestEdenConfig());
    overlay->initialize().get();
  }

  folly::test::TemporaryDirectory testDir_;
  std::shared_ptr<Overlay> overlay;
};

TEST_P(DebugDumpTreeOverlayInodesTest, dump_empty_directory) {
  auto ino = kRootNodeId;
  EXPECT_EQ(1_ino, ino);

  overlay->saveOverlayDir(ino, DirContents(kPathMapDefaultCaseSensitive));
  EXPECT_EQ(
      "/\n"
      "  Inode number: 1\n"
      "  Entries (0 total):\n",
      debugDumpOverlayInodes(*overlay, ino));
}

TEST_P(
    DebugDumpTreeOverlayInodesTest,
    dump_directory_with_an_empty_subdirectory) {
  auto rootIno = kRootNodeId;
  EXPECT_EQ(1_ino, rootIno);
  auto subdirIno = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, subdirIno);

  DirContents root(kPathMapDefaultCaseSensitive);
  root.emplace("subdir"_pc, S_IFDIR | 0755, subdirIno);
  overlay->saveOverlayDir(rootIno, root);

  overlay->saveOverlayDir(subdirIno, DirContents(kPathMapDefaultCaseSensitive));

  // At the time of writing, the TreeOverlay does not store mode, which is why
  // it is zero here
  EXPECT_EQ(
      "/\n"
      "  Inode number: 1\n"
      "  Entries (1 total):\n"
      "            2 d    0 subdir\n"
      "/subdir\n"
      "  Inode number: 2\n"
      "  Entries (0 total):\n",
      debugDumpOverlayInodes(*overlay, rootIno));
}

TEST_P(
    DebugDumpTreeOverlayInodesTest,
    dump_directory_with_unsaved_subdirectory) {
  auto rootIno = kRootNodeId;
  EXPECT_EQ(1_ino, rootIno);
  auto directoryDoesNotExistIno = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, directoryDoesNotExistIno);

  DirContents root(kPathMapDefaultCaseSensitive);
  root.emplace(
      "directory_does_not_exist"_pc, S_IFDIR | 0755, directoryDoesNotExistIno);
  overlay->saveOverlayDir(rootIno, root);

  // At the time of writing, the TreeOverlay does not store mode, which is why
  // it is zero here
  EXPECT_EQ(
      "/\n"
      "  Inode number: 1\n"
      "  Entries (1 total):\n"
      "            2 d    0 directory_does_not_exist\n"
      "/directory_does_not_exist\n"
      "  Inode number: 2\n"
      "  Entries (0 total):\n",
      debugDumpOverlayInodes(*overlay, rootIno));
}

TEST_P(
    DebugDumpTreeOverlayInodesTest,
    dump_directory_with_unsaved_regular_file) {
  auto rootIno = kRootNodeId;
  EXPECT_EQ(1_ino, rootIno);
  auto regularFileDoesNotExistIno = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, regularFileDoesNotExistIno);

  DirContents root(kPathMapDefaultCaseSensitive);
  root.emplace(
      "regular_file_does_not_exist"_pc,
      S_IFREG | 0644,
      regularFileDoesNotExistIno);
  overlay->saveOverlayDir(rootIno, root);

  // At the time of writing, the TreeOverlay does not store mode, which is why
  // it is zero here
  EXPECT_EQ(
      "/\n"
      "  Inode number: 1\n"
      "  Entries (1 total):\n"
      "            2 f    0 regular_file_does_not_exist\n",
      debugDumpOverlayInodes(*overlay, rootIno));
}

INSTANTIATE_TEST_SUITE_P(
    DebugDumpTreeOverlayInodesTest,
    DebugDumpTreeOverlayInodesTest,
    ::testing::Values(
        Overlay::OverlayType::Tree,
        Overlay::OverlayType::TreeBuffered));

} // namespace facebook::eden
