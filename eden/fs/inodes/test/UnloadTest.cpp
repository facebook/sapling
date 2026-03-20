/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include <thread>

#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/common/utils/FaultInjector.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/InodeUnloader.h"
#include "eden/fs/testharness/TestMount.h"

using namespace std::chrono_literals;
using namespace facebook::eden;

namespace {
template <typename Unloader>
struct UnloadTest : ::testing::Test {
  Unloader unloader;
};
} // namespace

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
TYPED_TEST_CASE(UnloadTest, InodeUnloaderTypes);
#pragma clang diagnostic pop

TYPED_TEST(UnloadTest, inodesAreUnloaded) {
  FakeTreeBuilder builder;
  builder.mkdir("docs");
  builder.setFile("docs/README.md", "readme");
  builder.setFile("docs/WholeFish", "sea bass");
  builder.mkdir("src");
  builder.setFile("src/code.c", "main() {}");
  builder.mkdir("test");
  builder.setFile("test/test.c", "TEST()");
  TestMount testMount{builder};

  const auto* edenMount = testMount.getEdenMount().get();
  auto inodeMap = edenMount->getInodeMap();

  std::vector<InodeNumber> loadedInodeNumbers;
  auto load = [&](RelativePathPiece relpath) -> InodeNumber {
    auto inode = testMount.getInode(relpath);
    inode->incFsRefcount();
    loadedInodeNumbers.push_back(inode->getNodeId());
    return inode->getNodeId();
  };

  // Load every file, increment the FUSE refcount, and remember its InodeNumber.
  auto readme_ino = load("docs/README.md"_relpath);
  auto wholefish_ino = load("docs/WholeFish"_relpath);
  auto code_ino = load("src/code.c"_relpath);
  auto test_ino = load("test/test.c"_relpath);

  EXPECT_TRUE(inodeMap->lookupInode(readme_ino).get());
  EXPECT_TRUE(inodeMap->lookupInode(wholefish_ino).get());
  EXPECT_TRUE(inodeMap->lookupInode(code_ino).get());
  EXPECT_TRUE(inodeMap->lookupInode(test_ino).get());

  // Now decrement the FUSE refcounts.
  inodeMap->decFsRefcount(readme_ino, 1);
  inodeMap->decFsRefcount(wholefish_ino, 1);
  inodeMap->decFsRefcount(code_ino, 1);
  inodeMap->decFsRefcount(test_ino, 1);

  // At this point, every file and tree should be loaded, plus the root and
  // .eden.
  // 4 files + 3 subdirectories + 1 root + 1 .eden + 4 .eden entries
  auto counts = inodeMap->getInodeCounts();
  EXPECT_EQ(5, counts.treeCount);
  EXPECT_EQ(8, counts.fileCount);
  EXPECT_EQ(0, counts.unloadedInodeCount);

  // Count includes files only, and the root's refcount will never go to zero
  // while the mount is up.
  EXPECT_EQ(12, this->unloader.unload(*edenMount->getRootInode()));

  counts = inodeMap->getInodeCounts();
  EXPECT_EQ(1, counts.treeCount);
  EXPECT_EQ(0, counts.fileCount);
  EXPECT_EQ(0, counts.unloadedInodeCount);
}

TYPED_TEST(UnloadTest, inodesCanBeUnloadedDuringLoad) {
  auto builder = FakeTreeBuilder{};
  builder.setFile("src/sub/file.txt", "this is a test file");
  TestMount testMount{builder, false};

  // Look up the "src" tree inode by name, which starts the load.
  // The future should only be fulfilled when after we make the tree ready
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto srcFuture =
      rootInode->getOrLoadChild("src"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(srcFuture.isReady());

  rootInode->unloadChildrenNow();

  builder.setReady("src");
  testMount.drainServerExecutor();
  ASSERT_TRUE(srcFuture.isReady());
  auto srcTree = std::move(srcFuture).get(1s).asTreePtr();
  EXPECT_NE(kRootNodeId, srcTree->getNodeId());

  auto subFuture =
      srcTree->getOrLoadChild("sub"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  srcTree.reset();
  EXPECT_FALSE(subFuture.isReady());

  rootInode->unloadChildrenNow();
  builder.setReady("src/sub");
  testMount.drainServerExecutor();
  ASSERT_TRUE(subFuture.isReady());

  auto sub = std::move(subFuture).get(1s);
  EXPECT_NE(kRootNodeId, sub->getNodeId());
}

TEST(UnloadAfterAsyncLoad, unloadRacesDeterministicallyWithInodeLoadComplete) {
  // This test deterministically reproduces the race between the background
  // unloader and async inode load completion by using fault injection to pause
  // inodeLoadComplete after the lock is released but before promises are
  // fulfilled. The fix (moving takeOwnership inside the lock) ensures
  // ptrAcquireCount_ > 0 during this window, so the unloader skips the inode.
  auto builder = FakeTreeBuilder{};
  builder.setFile("dir/subdir/file.txt", "test file contents");
  TestMount testMount{builder, false};

  auto& fi = testMount.getServerState()->getFaultInjector();
  fi.injectBlock("inodeLoadComplete", ".*");

  auto rootInode = testMount.getEdenMount()->getRootInode();

  // Start loading "dir". This creates the Promise<InodePtr> and starts the
  // async tree fetch from FakeBackingStore.
  auto dirFuture =
      rootInode->getOrLoadChild("dir"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());

  // Complete the backing store load on a background thread. The detached
  // executor is QueuedImmediateExecutor, so inodeLoadComplete runs inline
  // on this thread and blocks on the fault injection checkpoint.
  std::thread bgThread([&] { builder.setReady("dir"); });

  // Wait for inodeLoadComplete to hit the fault point. At this point:
  // - The inode is in loadedInodes_ and the DirEntry
  // - ptrAcquireCount_ == 1 (from takeOwnership inside the lock)
  // - The contents_ lock has been released
  // - Promises have NOT been fulfilled yet
  ASSERT_TRUE(fi.waitUntilBlocked("inodeLoadComplete", 10s));

  // Race: try to unload while inodeLoadComplete is paused. With the fix,
  // the unloader sees ptrAcquireCount_ == 1 and skips the inode. Without the
  // fix, ptrAcquireCount_ would be 0 here and the unloader would delete the
  // inode, causing a use-after-free when promises are fulfilled.
  rootInode->unloadChildrenNow();

  // Unblock to let promise fulfillment proceed.
  fi.unblock("inodeLoadComplete", ".*");
  fi.removeFault("inodeLoadComplete", ".*");
  bgThread.join();

  // Drive the executor to complete the ImmediateFuture callback chain.
  testMount.drainServerExecutor();

  // The future should resolve with a valid TreeInode (no use-after-free).
  ASSERT_TRUE(dirFuture.isReady());
  auto dirInode = std::move(dirFuture).get(1s).asTreePtr();
  EXPECT_NE(kRootNodeId, dirInode->getNodeId());
}

TEST(
    UnloadLastAccessedBefore,
    unloadsInodesByLastFsRequestTimeNotMetadataAtime) {
  FakeTreeBuilder builder;
  builder.setFile("old.txt", "old contents");
  builder.setFile("new.txt", "new contents");
  TestMount testMount{builder};

  const auto* edenMount = testMount.getEdenMount().get();
  auto inodeMap = edenMount->getInodeMap();

  // Load both files and give them FUSE references so they stay loaded
  auto oldInode = testMount.getInode("old.txt"_relpath);
  auto newInode = testMount.getInode("new.txt"_relpath);
  auto oldIno = oldInode->getNodeId();
  auto newIno = newInode->getNodeId();
  oldInode->incFsRefcount();
  newInode->incFsRefcount();

  // Advance the clock and touch only "new.txt" via updateLastFsRequestTime.
  // Both inodes start with lastFsRequestTime from mount creation.
  // After this, new.txt's lastFsRequestTime is 120s later than old.txt's.
  testMount.getClock().advance(120s);
  newInode->updateLastFsRequestTime();

  // Use a cutoff between old and new lastFsRequestTime values.
  auto cutoff = oldInode->getLastFsRequestTime().toTimespec();
  cutoff.tv_sec += 60;

  // Release InodePtrs (FUSE refcount keeps them alive in InodeMap)
  oldInode.reset();
  newInode.reset();

  // Drop FUSE references so inodes are eligible for unloading
  inodeMap->decFsRefcount(oldIno, 1);
  inodeMap->decFsRefcount(newIno, 1);

  auto countsBefore = inodeMap->getInodeCounts();

  auto rootInode = edenMount->getRootInode();
  auto unloaded = rootInode->unloadChildrenLastAccessedBefore(cutoff);

  // old.txt (lastFsRequestTime < cutoff) should be unloaded.
  // new.txt (lastFsRequestTime > cutoff) should remain loaded.
  EXPECT_GE(unloaded, 1);
  auto countsAfter = inodeMap->getInodeCounts();
  EXPECT_LT(countsAfter.fileCount, countsBefore.fileCount);

  // new.txt should still be loadable (still in loaded inodes)
  EXPECT_TRUE(inodeMap->lookupInode(newIno).get());
}

TEST(UnloadUnreferencedByFuse, inodesReferencedByFuseAreNotUnloaded) {
  FakeTreeBuilder builder;
  builder.mkdir("src");
  builder.setFile("src/file.txt", "contents");
  TestMount testMount{builder};

  const auto* edenMount = testMount.getEdenMount().get();
  auto inodeMap = edenMount->getInodeMap();

  auto inode = testMount.getInode("src/file.txt"_relpath);
  inode->incFsRefcount();
  inode.reset();

  // 1 file + 1 subdirectory + 1 root + 1 .eden + 4 .eden entries
  auto counts = inodeMap->getInodeCounts();
  EXPECT_EQ(3, counts.treeCount);
  EXPECT_EQ(5, counts.fileCount);
  EXPECT_EQ(0, counts.unloadedInodeCount);

  EXPECT_EQ(5, edenMount->getRootInode()->unloadChildrenUnreferencedByFs());

  // root + src + file.txt
  counts = inodeMap->getInodeCounts();
  EXPECT_EQ(2, counts.treeCount);
  EXPECT_EQ(1, counts.fileCount);
  EXPECT_EQ(0, counts.unloadedInodeCount);
}

#endif
