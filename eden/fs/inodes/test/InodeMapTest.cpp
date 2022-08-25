/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeMap.h"

#include <folly/String.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/Bug.h"

using namespace std::chrono_literals;
using namespace facebook::eden;

constexpr auto loadTimeoutLimit = 1000ms;
// we will wait up to loadTimeoutLimit, maxWaitForLoads times
// for EdenFS to finish loading all it's initial inodes.
constexpr auto maxWaitForLoads = 60;

#ifdef _WIN32
// TODO(puneetk): Defining these flags here to fix the linker issue. These
// symbols should come from ThriftProtocol.lib. But, for some reason it's not
// getting imported from the lib on Windows, even though it's linking against
// the lib.
DEFINE_int32(
    thrift_cpp2_protocol_reader_string_limit,
    0,
    "Limit on string size when deserializing thrift, 0 is no limit");
DEFINE_int32(
    thrift_cpp2_protocol_reader_container_limit,
    0,
    "Limit on container size when deserializing thrift, 0 is no limit");
#endif

TEST(InodeMap, invalidInodeNumber) {
  FakeTreeBuilder builder;
  builder.setFile("Makefile", "all:\necho success\n");
  builder.setFile("src/noop.c", "int main() { return 0; }\n");
  TestMount testMount{builder};

  EdenBugDisabler noCrash;
  auto* inodeMap = testMount.getEdenMount()->getInodeMap();
  auto future = inodeMap->lookupFileInode(0x12345678_ino);
  EXPECT_THROW_RE(
      std::move(future).get(), std::runtime_error, "unknown inode number");
}

TEST(InodeMap, simpleLookups) {
  // Test simple lookups that succeed immediately from the LocalStore
  FakeTreeBuilder builder;
  builder.setFile("Makefile", "all:\necho success\n");
  builder.setFile("src/noop.c", "int main() { return 0; }\n");
  TestMount testMount{builder};
  auto* inodeMap = testMount.getEdenMount()->getInodeMap();

  // Look up the tree inode by name first
  auto root = testMount.getEdenMount()->getRootInode();
  auto srcTreeFut =
      root->getOrLoadChild("src"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  auto srcTree = std::move(srcTreeFut).get(0ms);

  // Next look up the tree by inode number
  auto tree2Fut = inodeMap->lookupTreeInode(srcTree->getNodeId())
                      .semi()
                      .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  auto tree2 = std::move(tree2Fut).get(0ms);
  EXPECT_EQ(srcTree, tree2);
  EXPECT_EQ(RelativePath{"src"}, tree2->getPath());

  // Next look up src/noop.c by name
  auto noopFut =
      tree2->getOrLoadChild("noop.c"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  auto noop = std::move(noopFut).get(0ms);
  EXPECT_NE(srcTree->getNodeId(), noop->getNodeId());

  // And look up src/noop.c by inode ID
  auto noop2Fut = inodeMap->lookupFileInode(noop->getNodeId())
                      .semi()
                      .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  auto noop2 = std::move(noop2Fut).get(0ms);
  EXPECT_EQ(noop, noop2);
  EXPECT_EQ(RelativePath{"src/noop.c"}, noop2->getPath());

  // lookupTreeInode() and lookupFileInode() should fail
  // when called on the wrong file type.
  EXPECT_THROW_ERRNO(
      inodeMap->lookupFileInode(srcTree->getNodeId()).get(), EISDIR);
  EXPECT_THROW_ERRNO(
      inodeMap->lookupTreeInode(noop->getNodeId()).get(), ENOTDIR);
}

TEST(InodeMap, asyncLookup) {
  auto builder = FakeTreeBuilder();
  builder.setFile("README", "docs go here\n");
  builder.setFile("src/runme.sh", "#!/bin/sh\necho hello world\n", true);
  builder.setFile("src/test.txt", "this is a test file");
  TestMount testMount{builder, false};

  // Look up the "src" tree inode by name
  // The future should only be fulfilled when after we make the tree ready
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto srcFuture =
      rootInode->getOrLoadChild("src"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(srcFuture.isReady());

  // Start a second lookup before the first is ready
  auto srcFuture2 =
      rootInode->getOrLoadChild("src"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(srcFuture2.isReady());

  // Now make the tree ready
  builder.setReady("src");
  testMount.drainServerExecutor();
  ASSERT_TRUE(srcFuture.isReady());
  ASSERT_TRUE(srcFuture2.isReady());
  auto srcTree = std::move(srcFuture).get(std::chrono::seconds(1));
  auto srcTree2 = std::move(srcFuture2).get(std::chrono::seconds(1));
  EXPECT_EQ(srcTree.get(), srcTree2.get());
}

TEST(InodeMap, asyncError) {
  auto builder = FakeTreeBuilder();
  builder.setFile("README", "docs go here\n");
  builder.setFile("src/runme.sh", "#!/bin/sh\necho hello world\n", true);
  builder.setFile("src/test.txt", "this is a test file");
  TestMount testMount{builder, false};

  // Look up the "src" tree inode by name
  // The future should only be fulfilled when after we make the tree ready
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto srcFuture =
      rootInode->getOrLoadChild("src"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(srcFuture.isReady());

  // Start a second lookup before the first is ready
  auto srcFuture2 =
      rootInode->getOrLoadChild("src"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(srcFuture2.isReady());

  // Now fail the tree lookup
  builder.triggerError(
      "src", std::domain_error("rejecting lookup for src tree"));
  testMount.drainServerExecutor();
  ASSERT_TRUE(srcFuture.isReady());
  ASSERT_TRUE(srcFuture2.isReady());
  EXPECT_THROW(std::move(srcFuture).get(), std::domain_error);
  EXPECT_THROW(std::move(srcFuture2).get(), std::domain_error);
}

TEST(InodeMap, recursiveLookup) {
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/c/d/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();

  // Call EdenMount::getInodeSlow() to do a recursive lookup
  auto fileFuture =
      edenMount
          ->getInodeSlow(
              "a/b/c/d/file.txt"_relpath, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a/b/c");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b/c/d/file.txt");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b/c/d");
  testMount.drainServerExecutor();
  ASSERT_TRUE(fileFuture.isReady());
  auto fileInode = std::move(fileFuture).get();
  EXPECT_EQ("a/b/c/d/file.txt"_relpath, fileInode->getPath().value());
}

TEST(InodeMap, recursiveLookupError) {
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/c/d/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();

  // Call EdenMount::getInodeSlow() to do a recursive lookup
  auto fileFuture =
      edenMount
          ->getInodeSlow(
              "a/b/c/d/file.txt"_relpath, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b/c");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b/c/d/file.txt");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.triggerError(
      "a/b/c/d", std::domain_error("error for testing purposes"));
  testMount.drainServerExecutor();
  ASSERT_TRUE(fileFuture.isReady());
  EXPECT_THROW_RE(
      std::move(fileFuture).get(),
      std::domain_error,
      "error for testing purposes");
}

TEST(InodeMap, renameDuringRecursiveLookup) {
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/c/d/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();

  // Call EdenMount::getInodeSlow() to do a recursive lookup
  auto fileFuture =
      edenMount
          ->getInodeSlow(
              "a/b/c/d/file.txt"_relpath, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a/b/c");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());

  auto bInode = testMount.getTreeInode("a/b"_relpath);

  // Rename c to x after the recursive resolution should have
  // already looked it up
  auto renameFuture = bInode
                          ->rename(
                              "c"_pc,
                              bInode,
                              "x"_pc,
                              InvalidationRequired::No,
                              ObjectFetchContext::getNullContext())
                          .semi()
                          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(fileFuture.isReady());

  // Now mark the rest of the tree ready
  // Note that we don't actually have to mark the file itself ready.
  // The Inode lookup itself doesn't need the blob data yet.
  builder.setReady("a/b/c/d");
  testMount.drainServerExecutor();
  ASSERT_TRUE(fileFuture.isReady());
  auto fileInode = std::move(fileFuture).get();
  // We should have successfully looked up the inode, but it will report it
  // self (correctly) at its new path now.
  EXPECT_EQ("a/b/x/d/file.txt"_relpath, fileInode->getPath().value());
}

TEST(InodeMap, renameDuringRecursiveLookupAndLoad) {
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/c/d/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();

  // Call EdenMount::getInodeSlow() to do a recursive lookup
  auto fileFuture =
      edenMount
          ->getInodeSlow(
              "a/b/c/d/file.txt"_relpath, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b");
  testMount.drainServerExecutor();
  EXPECT_FALSE(fileFuture.isReady());

  auto bInode = testMount.getTreeInode("a/b"_relpath);

  // Rename c to x while the recursive resolution is still trying
  // to look it up.
  auto renameFuture = bInode
                          ->rename(
                              "c"_pc,
                              bInode,
                              "x"_pc,
                              InvalidationRequired::No,
                              ObjectFetchContext::getNullContext())
                          .semi()
                          .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  // The rename will not complete until C becomes ready
  EXPECT_FALSE(renameFuture.isReady());
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a/b/c");
  testMount.drainServerExecutor();
  EXPECT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(fileFuture.isReady());

  // Now mark the rest of the tree ready
  // Note that we don't actually have to mark the file itself ready.
  // The Inode lookup itself doesn't need the blob data yet.
  builder.setReady("a/b/c/d");
  testMount.drainServerExecutor();
  ASSERT_TRUE(fileFuture.isReady());
  auto fileInode = std::move(fileFuture).get();
  // We should have successfully looked up the inode, but it will report it
  // self (correctly) at its new path now.
  EXPECT_EQ("a/b/x/d/file.txt"_relpath, fileInode->getPath().value());
}

// Tests InodeMap::lookupInode when loading an unloaded inode by inode number
TEST(InodeMap, lookingUpAnUnloadedInodeAddsLoadsToTraceBus) {
  folly::UnboundedQueue<InodeTraceEvent, true, true, false> queue;
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();
  auto* inodeMap = edenMount->getInodeMap();
  auto& trace_bus = edenMount->getInodeTraceBus();

  // Detect inode load events and add events to synchronized queue
  auto handle = trace_bus.subscribeFunction(
      folly::to<std::string>("inodeMapTest-", edenMount->getPath().basename()),
      [&](const InodeTraceEvent& event) {
        if (event.eventType == InodeEventType::LOAD) {
          std::cout << "Event: " << event.getPath() << " " << event.ino << " "
                    << ((event.progress == InodeEventProgress::END) ? "End"
                                                                    : "Start")
                    << std::endl;
          queue.enqueue(event);
        }
      });

  // Wait for any initial load events to complete
  size_t iteration_count = 0;
  while (queue.try_dequeue_for(loadTimeoutLimit).has_value() &&
         iteration_count < maxWaitForLoads) {
    ++iteration_count;
    if (iteration_count >= maxWaitForLoads) {
      throw std::runtime_error{
          "EdenFS not settling after startup, too many loads"};
    }
  };

  // In order to get inode numbers, we load "a" and "a/b" by path
  auto root = edenMount->getRootInode();
  auto a_future =
      edenMount->getInodeSlow("a"_relpath, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  auto b_future =
      edenMount
          ->getInodeSlow("a/b"_relpath, ObjectFetchContext::getNullContext())
          .semi()
          .via(testMount.getServerExecutor().get());
  builder.setReady("a");
  builder.setReady("a/b");
  testMount.drainServerExecutor();

  // Get inode numbers and move inodes to the InodeMap's UnloadedInodes
  InodeNumber an, bn;
  {
    auto a = std::move(a_future).get(0ms).asTreePtr();
    an = a->getNodeId();
    a->incFsRefcount();
    {
      auto b = std::move(b_future).get(0ms);
      bn = b->getNodeId();
      b->incFsRefcount();
    }
    a->unloadChildrenNow(); // Unloads b
  }
  root->unloadChildrenNow(); // Unloads a

  // With inode numbers, we ensure the initial loads came in expected order
  EXPECT_TRUE(isInodeMaterializedInQueue(queue, InodeEventProgress::START, an));
  EXPECT_TRUE(isInodeMaterializedInQueue(queue, InodeEventProgress::END, an));
  EXPECT_TRUE(isInodeMaterializedInQueue(queue, InodeEventProgress::START, bn));
  EXPECT_TRUE(isInodeMaterializedInQueue(queue, InodeEventProgress::END, bn));

  // Finally, we lookup the inodes by InodeNumber
  auto secondAFuture = inodeMap->lookupTreeInode(an).semi().via(
      testMount.getServerExecutor().get());
  auto secondBFuture = inodeMap->lookupTreeInode(bn).semi().via(
      testMount.getServerExecutor().get());
  testMount.drainServerExecutor();
  std::move(secondAFuture).get(0ms);
  std::move(secondBFuture).get(0ms);
  EXPECT_TRUE(isInodeMaterializedInQueue(queue, InodeEventProgress::START, an));
  EXPECT_TRUE(isInodeMaterializedInQueue(queue, InodeEventProgress::END, an));
  EXPECT_TRUE(isInodeMaterializedInQueue(queue, InodeEventProgress::START, bn));
  EXPECT_TRUE(isInodeMaterializedInQueue(queue, InodeEventProgress::END, bn));

  // Ensure we do not count any other loads a second time
  EXPECT_FALSE(queue.try_dequeue_for(loadTimeoutLimit).has_value());
}

TEST(InodeMap, unloadedUnlinkedTreesAreRemovedFromOverlay) {
  FakeTreeBuilder builder;
  builder.setFile("dir1/file.txt", "contents");
  builder.setFile("dir2/file.txt", "contents");
  TestMount mount{builder};
  auto edenMount = mount.getEdenMount();

  auto root = edenMount->getRootInode();
  auto dir1 = mount.getTreeInode("dir1"_relpath);
  auto dir2 = mount.getTreeInode("dir2"_relpath);
  auto dir1ino = dir1->getNodeId();
  auto dir2ino = dir2->getNodeId();

  auto fut = dir1->unlink(
                     "file.txt"_pc,
                     InvalidationRequired::No,
                     ObjectFetchContext::getNullContext())
                 .semi()
                 .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();
  std::move(fut).get(0ms);
  fut = dir2->unlink(
                "file.txt"_pc,
                InvalidationRequired::No,
                ObjectFetchContext::getNullContext())
            .semi()
            .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();
  std::move(fut).get(0ms);

  // Test both having a positive and zero fuse reference counts.
  dir2->incFsRefcount();

  fut = root->rmdir(
                "dir1"_pc,
                InvalidationRequired::No,
                ObjectFetchContext::getNullContext())
            .semi()
            .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();
  std::move(fut).get(0ms);
  fut = root->rmdir(
                "dir2"_pc,
                InvalidationRequired::No,
                ObjectFetchContext::getNullContext())
            .semi()
            .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();
  std::move(fut).get(0ms);

  dir1.reset();
  dir2.reset();

  edenMount->getInodeMap()->decFsRefcount(dir2ino);
  EXPECT_FALSE(mount.hasOverlayData(dir1ino));
  EXPECT_FALSE(mount.hasOverlayData(dir2ino));
#ifndef _WIN32
  EXPECT_FALSE(mount.hasMetadata(dir1ino));
  EXPECT_FALSE(mount.hasMetadata(dir2ino));
#endif // !_WIN32
}

#ifndef _WIN32

TEST(InodeMap, unloadedFileMetadataIsForgotten) {
  FakeTreeBuilder builder;
  builder.setFile("dir1/file.txt", "contents");
  builder.setFile("dir2/file.txt", "contents");
  TestMount mount{builder};
  auto edenMount = mount.getEdenMount();

  auto root = edenMount->getRootInode();
  auto dir1 = mount.getTreeInode("dir1"_relpath);
  auto dir2 = mount.getTreeInode("dir2"_relpath);

  auto file1 = mount.getFileInode("dir1/file.txt"_relpath);
  auto file1ino = file1->getNodeId();
  auto file2 = mount.getFileInode("dir2/file.txt"_relpath);
  auto file2ino = file2->getNodeId();

  EXPECT_TRUE(mount.hasMetadata(file1ino));
  EXPECT_TRUE(mount.hasMetadata(file2ino));

  // Try having both positive and zero FUSE reference counts.
  file1->incFsRefcount();
  file1.reset();
  file2.reset();

  dir1->unlink(
          PathComponentPiece{"file.txt"},
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get(0ms);
  dir2->unlink(
          PathComponentPiece{"file.txt"},
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_TRUE(mount.hasMetadata(file1ino));
  EXPECT_FALSE(mount.hasMetadata(file2ino));

  edenMount->getInodeMap()->decFsRefcount(file1ino);
  EXPECT_FALSE(mount.hasMetadata(file1ino));
  EXPECT_FALSE(mount.hasMetadata(file2ino));
}
#endif

struct InodePersistenceTreeTest : ::testing::Test {
  InodePersistenceTreeTest() {
    builder.setFile("dir/file1.txt", "contents1");
    builder.setFile("dir/file2.txt", "contents2");
  }

  FakeTreeBuilder builder;
};

struct InodePersistenceTakeoverTest : InodePersistenceTreeTest {
  InodePersistenceTakeoverTest()
      : testMount{builder}, edenMount{testMount.getEdenMount()} {}

  void SetUp() override {
    InodePersistenceTreeTest::SetUp();

    auto tree = testMount.getInode("dir"_relpath);
    auto file1 = testMount.getInode("dir/file1.txt"_relpath);
    auto file2 = testMount.getInode("dir/file2.txt"_relpath);

    // Pretend FUSE is keeping references to these.
    tree->incFsRefcount();
    file1->incFsRefcount();
    file2->incFsRefcount();

    oldTreeId = tree->getNodeId();
    oldFile1Id = file1->getNodeId();
    oldFile2Id = file2->getNodeId();

    edenMount.reset();
    tree.reset();
    file1.reset();
    file2.reset();
#ifdef _WIN32
    // Windows doesn't support graceful restart yet. Here these tests help
    // test the consistency of the overlay. On Windows we are using Sqlite
    // Overlay which maintains the same inode number for each inode, after
    // remounts.
    testMount.remount();
#else
    testMount.remountGracefully();
#endif
    edenMount = testMount.getEdenMount();
  }

  TestMount testMount;
  std::shared_ptr<EdenMount> edenMount;

  InodeNumber oldTreeId;
  InodeNumber oldFile1Id;
  InodeNumber oldFile2Id;
};

TEST_F(
    InodePersistenceTakeoverTest,
    preservesInodeNumbersForLoadedInodesDuringTakeover_lookupFirstByName) {
  // Look up in a different order to avoid allocating the same numbers.
  auto tree = testMount.getInode("dir"_relpath);
  auto file2 = testMount.getInode("dir/file2.txt"_relpath);
  auto file1 = testMount.getInode("dir/file1.txt"_relpath);

#ifndef _WIN32
  EXPECT_EQ(1, tree->debugGetFsRefcount());
  EXPECT_EQ(1, file1->debugGetFsRefcount());
  EXPECT_EQ(1, file2->debugGetFsRefcount());
#endif // !1

  EXPECT_EQ(oldTreeId, tree->getNodeId());
  EXPECT_EQ(oldFile1Id, file1->getNodeId());
  EXPECT_EQ(oldFile2Id, file2->getNodeId());

  // Now try looking up by inode number.
  EXPECT_EQ(
      "dir",
      edenMount->getInodeMap()->lookupInode(oldTreeId).get()->getLogPath());
  EXPECT_EQ(
      "dir/file1.txt",
      edenMount->getInodeMap()->lookupInode(oldFile1Id).get()->getLogPath());
  EXPECT_EQ(
      "dir/file2.txt",
      edenMount->getInodeMap()->lookupInode(oldFile2Id).get()->getLogPath());
}

// The following test will not work on Windows, because on Windows we remount
// instead of remountGracefully and remount doesn't pre-populate the InodeMap.
// The lookupFirstByName above will work because checking by name will populate
// the InodeMap for us.

#ifndef _WIN32
TEST_F(
    InodePersistenceTakeoverTest,
    preservesInodeNumbersForLoadedInodesDuringTakeover_lookupFirstByNumber) {
  // Look up by number first.
  auto oldTreeIdFut =
      edenMount->getInodeMap()->lookupInode(oldTreeId).semi().via(
          testMount.getServerExecutor().get());
  auto oldFile1IdFut = edenMount->getInodeMap()
                           ->lookupInode(oldFile1Id)
                           .semi()
                           .via(testMount.getServerExecutor().get());
  auto oldFile2IdFut = edenMount->getInodeMap()
                           ->lookupInode(oldFile2Id)
                           .semi()
                           .via(testMount.getServerExecutor().get());
  testMount.drainServerExecutor();

  EXPECT_EQ("dir", std::move(oldTreeIdFut).get(0ms)->getLogPath());
  EXPECT_EQ("dir/file1.txt", std::move(oldFile1IdFut).get(0ms)->getLogPath());
  EXPECT_EQ("dir/file2.txt", std::move(oldFile2IdFut).get(0ms)->getLogPath());

  // Verify the same inodes can be looked up by name too.
  auto tree = testMount.getInode("dir"_relpath);
  auto file2 = testMount.getInode("dir/file2.txt"_relpath);
  auto file1 = testMount.getInode("dir/file1.txt"_relpath);

#ifndef _WIN32
  EXPECT_EQ(1, tree->debugGetFsRefcount());
  EXPECT_EQ(1, file1->debugGetFsRefcount());
  EXPECT_EQ(1, file2->debugGetFsRefcount());
#endif // !_WIN32

  EXPECT_EQ(oldTreeId, tree->getNodeId());
  EXPECT_EQ(oldFile1Id, file1->getNodeId());
  EXPECT_EQ(oldFile2Id, file2->getNodeId());
}
#endif

/**
 * clang and gcc use the inode number of a header to determine whether it's the
 * same file as one previously included and marked #pragma once.
 *
 * At least as long as the mount is up (including though graceful takeovers),
 * Eden must provide consistent inode numbers.
 */
TEST_F(
    InodePersistenceTreeTest,
    preservesInodeNumbersForUnloadedInodesDuringTakeover) {
  TestMount testMount{builder};
  auto edenMount = testMount.getEdenMount();

  auto tree = testMount.getInode("dir"_relpath);
  auto file1 = testMount.getInode("dir/file1.txt"_relpath);
  auto file2 = testMount.getInode("dir/file2.txt"_relpath);

  tree->incFsRefcount();
  file1->incFsRefcount();
  file2->incFsRefcount();

  auto oldTreeId = tree->getNodeId();
  auto oldFile1Id = file1->getNodeId();
  auto oldFile2Id = file2->getNodeId();

  tree->decFsRefcount();
  file1->decFsRefcount();
  file2->decFsRefcount();

  edenMount.reset();
  tree.reset();
  file1.reset();
  file2.reset();
#ifdef _WIN32
  // Windows doesn't support graceful restart yet. Here these tests help
  // test the consistency of the overlay. On Windows we are using Sqlite
  // Overlay which maintains the same inode number for each inode, after
  // remounts.
  testMount.remount();
#else
  testMount.remountGracefully();
#endif

  edenMount = testMount.getEdenMount();

  // Look up in a different order.
  tree = testMount.getInode("dir"_relpath);
  file2 = testMount.getInode("dir/file2.txt"_relpath);
  file1 = testMount.getInode("dir/file1.txt"_relpath);

  EXPECT_EQ(oldTreeId, tree->getNodeId());
  EXPECT_EQ(oldFile1Id, file1->getNodeId());
  EXPECT_EQ(oldFile2Id, file2->getNodeId());
}
