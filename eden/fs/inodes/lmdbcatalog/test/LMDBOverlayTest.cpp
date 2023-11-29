/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/lmdbcatalog/BufferedLMDBInodeCatalog.h"
#include "eden/fs/inodes/lmdbcatalog/LMDBInodeCatalog.h"
#include "eden/fs/inodes/test/OverlayTestUtil.h"

#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/testharness/TestMount.h"

namespace facebook::eden {

class LMDBOverlayTest : public ::testing::TestWithParam<InodeCatalogOptions> {
 protected:
  InodeCatalogOptions overlayOptions() const {
    return GetParam();
  }

  void SetUp() override {
    // Set up a directory structure that we will use for most
    // of the tests below
    FakeTreeBuilder builder;
    builder.mkdir("dir");
    builder.mkdir("foo");
    builder.mkdir("foo/bar");
    mount_.initialize(builder, InodeCatalogType::LMDB, overlayOptions());
  }

  TestMount mount_;
};

TEST_P(LMDBOverlayTest, roundTripThroughSaveAndLoad) {
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
    LMDBOverlayTest,
    LMDBOverlayTest,
    ::testing::Values(INODE_CATALOG_DEFAULT, INODE_CATALOG_BUFFERED));

TEST(PlainLMDBOverlayTest, new_overlay_is_clean) {
  folly::test::TemporaryDirectory testDir;
  auto overlay = Overlay::create(
      canonicalPath(testDir.path().string()),
      kPathMapDefaultCaseSensitive,
      InodeCatalogType::LMDB,
      INODE_CATALOG_DEFAULT,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      true,
      *EdenConfig::createTestEdenConfig());
  overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  EXPECT_TRUE(overlay->hadCleanStartup());
}

TEST(PlainLMDBOverlayTest, new_overlay_is_clean_buffered) {
  folly::test::TemporaryDirectory testDir;
  auto overlay = Overlay::create(
      canonicalPath(testDir.path().string()),
      kPathMapDefaultCaseSensitive,
      InodeCatalogType::LMDB,
      INODE_CATALOG_BUFFERED,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      true,
      *EdenConfig::createTestEdenConfig());
  overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  EXPECT_TRUE(overlay->hadCleanStartup());
}

TEST(PlainLMDBOverlayTest, reopened_overlay_is_clean) {
  folly::test::TemporaryDirectory testDir;
  {
    auto overlay = Overlay::create(
        canonicalPath(testDir.path().string()),
        kPathMapDefaultCaseSensitive,
        InodeCatalogType::LMDB,
        INODE_CATALOG_DEFAULT,
        std::make_shared<NullStructuredLogger>(),
        makeRefPtr<EdenStats>(),
        true,
        *EdenConfig::createTestEdenConfig());
    overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  }
  auto overlay = Overlay::create(
      canonicalPath(testDir.path().string()),
      kPathMapDefaultCaseSensitive,
      InodeCatalogType::LMDB,
      INODE_CATALOG_DEFAULT,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      true,
      *EdenConfig::createTestEdenConfig());
  overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  EXPECT_TRUE(overlay->hadCleanStartup());
}

TEST(PlainLMDBOverlayTest, reopened_overlay_is_clean_buffered) {
  folly::test::TemporaryDirectory testDir;
  {
    auto overlay = Overlay::create(
        canonicalPath(testDir.path().string()),
        kPathMapDefaultCaseSensitive,
        InodeCatalogType::LMDB,
        INODE_CATALOG_BUFFERED,
        std::make_shared<NullStructuredLogger>(),
        makeRefPtr<EdenStats>(),
        true,
        *EdenConfig::createTestEdenConfig());
    overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  }
  auto overlay = Overlay::create(
      canonicalPath(testDir.path().string()),
      kPathMapDefaultCaseSensitive,
      InodeCatalogType::LMDB,
      INODE_CATALOG_BUFFERED,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      true,
      *EdenConfig::createTestEdenConfig());
  overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  EXPECT_TRUE(overlay->hadCleanStartup());
}

TEST(PlainLMDBOverlayTest, close_overlay_with_no_capacity_buffered) {
  auto config = EdenConfig::createTestEdenConfig();
  config->overlayBufferSize.setValue(0, ConfigSourceType::Default, true);
  folly::test::TemporaryDirectory testDir;
  auto overlay = Overlay::create(
      canonicalPath(testDir.path().string()),
      kPathMapDefaultCaseSensitive,
      InodeCatalogType::LMDB,
      INODE_CATALOG_BUFFERED,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      true,
      *config);
  overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  overlay->close();
  EXPECT_TRUE(overlay->isClosed());
}

TEST(PlainLMDBOverlayTest, small_capacity_write_multiple_directories_buffered) {
  auto config = EdenConfig::createTestEdenConfig();
  config->overlayBufferSize.setValue(1, ConfigSourceType::Default, true);
  folly::test::TemporaryDirectory testDir;
  auto overlay = Overlay::create(
      canonicalPath(testDir.path().string()),
      kPathMapDefaultCaseSensitive,
      InodeCatalogType::LMDB,
      INODE_CATALOG_BUFFERED,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      true,
      *config);
  overlay->initialize(EdenConfig::createTestEdenConfig()).get();

  EXPECT_EQ(kRootNodeId, overlay->getMaxInodeNumber());

  DirContents dir(kPathMapDefaultCaseSensitive);
  InodeNumber ino;

  // 20 iterations is an arbitrary choice. With the buffer size set to 1 byte,
  // the worker thread will process events one-by-one, and 20 here gives a good
  // chance of getting more than one write queued
  for (int i = 0; i < 20; i++) {
    ino = overlay->allocateInodeNumber();
    overlay->saveOverlayDir(ino, dir);
  }

  EXPECT_EQ(ino, overlay->getMaxInodeNumber());
}

class RawLMDBOverlayTest
    : public ::testing::TestWithParam<InodeCatalogOptions> {
 public:
  RawLMDBOverlayTest() : testDir_{makeTempDir("eden_raw_overlay_test_")} {
    loadOverlay();
  }

  InodeCatalogOptions overlayOptions() const {
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
        InodeCatalogType::LMDB,
        overlayOptions(),
        std::make_shared<NullStructuredLogger>(),
        makeRefPtr<EdenStats>(),
        true,
        *EdenConfig::createTestEdenConfig());
    overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  }

  AbsolutePath getLocalDir() {
    return canonicalPath(testDir_.path().string());
  }

  folly::test::TemporaryDirectory testDir_;
  std::shared_ptr<Overlay> overlay;
};

TEST_P(RawLMDBOverlayTest, cannot_save_overlay_dir_when_closed) {
  overlay->close();
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);

  DirContents dir(kPathMapDefaultCaseSensitive);
  EXPECT_THROW_RE(
      overlay->saveOverlayDir(ino2, dir),
      std::system_error,
      "cannot access overlay after it is closed");
}

TEST_P(RawLMDBOverlayTest, max_inode_number_is_1_if_overlay_is_empty) {
  EXPECT_EQ(kRootNodeId, overlay->getMaxInodeNumber());
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);

  recreate();

  EXPECT_EQ(kRootNodeId, overlay->getMaxInodeNumber());
  ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);

  DirContents dir(kPathMapDefaultCaseSensitive);
  overlay->saveOverlayDir(2_ino, dir);

  recreate();

  EXPECT_EQ(2_ino, overlay->getMaxInodeNumber());
}

TEST_P(RawLMDBOverlayTest, remembers_max_inode_number_of_tree_entries) {
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

TEST_P(RawLMDBOverlayTest, inode_numbers_after_takeover) {
  auto ino2 = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, ino2);
  auto ino3 = overlay->allocateInodeNumber();
  auto ino4 = overlay->allocateInodeNumber();
  auto ino5 = overlay->allocateInodeNumber();

  // Write a subdir.
  DirContents subdir(kPathMapDefaultCaseSensitive);
  subdir.emplace(PathComponentPiece{"f"}, S_IFREG | 0644, ino5);
  overlay->saveOverlayDir(ino2, subdir);

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

TEST_P(RawLMDBOverlayTest, manual_recursive_delete) {
  auto rootIno = kRootNodeId;
  EXPECT_EQ(1_ino, rootIno);
  auto subdirIno = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, subdirIno);
  auto subdirIno2 = overlay->allocateInodeNumber();
  EXPECT_EQ(3_ino, subdirIno2);

  DirContents rootContents(kPathMapDefaultCaseSensitive);
  auto rootChildEntry =
      rootContents.emplace("subdir"_pc, S_IFDIR | 0755, subdirIno);
  // equivalent to overlay->saveOverlayDir(rootIno, rootContents);
  overlay->addChild(rootIno, *rootChildEntry.first, rootContents);

  DirContents subdirContents(kPathMapDefaultCaseSensitive);
  auto subdirChildEntry =
      subdirContents.emplace("subdir2"_pc, S_IFDIR | 0755, subdirIno2);
  // equivalent to overlay->saveOverlayDir(subdirIno, subdirContents);
  overlay->addChild(subdirIno, *subdirChildEntry.first, subdirContents);

  DirContents subdir2Contents(kPathMapDefaultCaseSensitive);
  overlay->saveOverlayDir(subdirIno2, subdir2Contents);

  if (overlayOptions() == INODE_CATALOG_BUFFERED) {
    // Empty the write queue
    static_cast<BufferedLMDBInodeCatalog*>(overlay->getRawInodeCatalog())
        ->flush();

    folly::Promise<folly::Unit> promise;
    SCOPE_EXIT {
      // Unblock the queue to allow the test to finish
      promise.setValue(folly::unit);
    };
    auto fut = promise.getFuture();

    // Pause the BufferedLMDBInodeCatalog worker thread so we can force
    // loadAndRemoveOverlayDir to serve the read from the write queue
    static_cast<BufferedLMDBInodeCatalog*>(overlay->getRawInodeCatalog())
        ->pause(std::move(fut));

    // Resave the overlayDir so the data is in the write queue
    overlay->saveOverlayDir(subdirIno, subdirContents);

    // This call will fall fail to find the data in the write queue and will
    // fall back to calling LMDBInodeCatalog::loadAndRemoveOverlayDir
    // synchronously
    static_cast<BufferedLMDBInodeCatalog*>(overlay->getRawInodeCatalog())
        ->loadAndRemoveOverlayDir(subdirIno2);

    // This call will serve the load from the in-memory write queue
    static_cast<BufferedLMDBInodeCatalog*>(overlay->getRawInodeCatalog())
        ->loadAndRemoveOverlayDir(subdirIno);
  } else {
    overlay->saveOverlayDir(subdirIno, subdirContents);
    static_cast<LMDBInodeCatalog*>(overlay->getRawInodeCatalog())
        ->loadAndRemoveOverlayDir(subdirIno2);
    static_cast<LMDBInodeCatalog*>(overlay->getRawInodeCatalog())
        ->loadAndRemoveOverlayDir(subdirIno);
  }
}

INSTANTIATE_TEST_SUITE_P(
    RawLMDBOverlayTest,
    RawLMDBOverlayTest,
    ::testing::Values(INODE_CATALOG_DEFAULT, INODE_CATALOG_BUFFERED));

class DebugDumpLMDBOverlayInodesTest
    : public ::testing::TestWithParam<InodeCatalogOptions> {
 public:
  InodeCatalogOptions overlayOptions() const {
    return GetParam();
  }

  DebugDumpLMDBOverlayInodesTest()
      : testDir_{makeTempDir("eden_DebugDumpLMDBOverlayInodesTest")} {
    overlay = Overlay::create(
        canonicalPath(testDir_.path().string()),
        kPathMapDefaultCaseSensitive,
        InodeCatalogType::LMDB,
        overlayOptions(),
        std::make_shared<NullStructuredLogger>(),
        makeRefPtr<EdenStats>(),
        true,
        *EdenConfig::createTestEdenConfig());
    overlay->initialize(EdenConfig::createTestEdenConfig()).get();
  }

  void flush() {
    if (overlayOptions() == INODE_CATALOG_BUFFERED) {
      static_cast<BufferedLMDBInodeCatalog*>(overlay->getRawInodeCatalog())
          ->flush();
      // A second flush is needed here to ensure the worker thread has a chance
      // to acquire the state_ lock and clear the inflightOperation map in the
      // case that the first flush was was processed during the same iteration
      // as outstanding writes
      static_cast<BufferedLMDBInodeCatalog*>(overlay->getRawInodeCatalog())
          ->flush();
    }
  }

  folly::test::TemporaryDirectory testDir_;
  std::shared_ptr<Overlay> overlay;
};

TEST_P(DebugDumpLMDBOverlayInodesTest, dump_empty_directory) {
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
    DebugDumpLMDBOverlayInodesTest,
    dump_directory_with_an_empty_subdirectory) {
  auto rootIno = kRootNodeId;
  EXPECT_EQ(1_ino, rootIno);
  auto subdirIno = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, subdirIno);

  DirContents root(kPathMapDefaultCaseSensitive);
  root.emplace("subdir"_pc, S_IFDIR | 0755, subdirIno);
  overlay->saveOverlayDir(rootIno, root);

  overlay->saveOverlayDir(subdirIno, DirContents(kPathMapDefaultCaseSensitive));

  // The results can be different if the overlay is read from the write queue or
  // from disk since we don't store mode, the flush here makes the tests
  // deterministic
  flush();

  // At the time of writing, the LMDBInodeCatalog does not store mode, which
  // is why it is zero here
  EXPECT_EQ(
      "/\n"
      "  Inode number: 1\n"
      "  Entries (1 total):\n"
      "            2 d  755 subdir\n"
      "/subdir\n"
      "  Inode number: 2\n"
      "  Entries (0 total):\n",
      debugDumpOverlayInodes(*overlay, rootIno));
}

TEST_P(
    DebugDumpLMDBOverlayInodesTest,
    dump_directory_with_unsaved_subdirectory) {
  auto rootIno = kRootNodeId;
  EXPECT_EQ(1_ino, rootIno);
  auto directoryDoesNotExistIno = overlay->allocateInodeNumber();
  EXPECT_EQ(2_ino, directoryDoesNotExistIno);

  DirContents root(kPathMapDefaultCaseSensitive);
  root.emplace(
      "directory_does_not_exist"_pc, S_IFDIR | 0755, directoryDoesNotExistIno);
  overlay->saveOverlayDir(rootIno, root);

  // The results can be different if the overlay is read from the write queue or
  // from disk since we don't store mode, the flush here makes the tests
  // deterministic
  flush();

  // At the time of writing, the LMDBInodeCatalog does not store mode, which
  // is why it is zero here
  EXPECT_EQ(
      "/\n"
      "  Inode number: 1\n"
      "  Entries (1 total):\n"
      "            2 d  755 directory_does_not_exist\n"
      "/directory_does_not_exist\n"
      "  Inode number: 2\n"
      "  Entries (0 total):\n",
      debugDumpOverlayInodes(*overlay, rootIno));
}

TEST_P(
    DebugDumpLMDBOverlayInodesTest,
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

  // The results can be different if the overlay is read from the write queue or
  // from disk since we don't store mode, the flush here makes the tests
  // deterministic
  flush();

  // At the time of writing, the LMDBInodeCatalog does not store mode, which
  // is why it is zero here
  EXPECT_EQ(
      "/\n"
      "  Inode number: 1\n"
      "  Entries (1 total):\n"
      "            2 f  644 regular_file_does_not_exist\n",
      debugDumpOverlayInodes(*overlay, rootIno));
}

INSTANTIATE_TEST_SUITE_P(
    DebugDumpLMDBOverlayInodesTest,
    DebugDumpLMDBOverlayInodesTest,
    ::testing::Values(INODE_CATALOG_DEFAULT, INODE_CATALOG_BUFFERED));

} // namespace facebook::eden
