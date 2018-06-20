/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/EdenMount.h"

#include <folly/Range.h>
#include <folly/chrono/Conv.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace std::chrono_literals;
using folly::Optional;
using namespace facebook::eden;

TEST(EdenMount, initFailure) {
  // Test initializing an EdenMount with a commit hash that does not exist.
  // This should fail with an exception, and not crash.
  TestMount testMount;
  EXPECT_THROW_RE(
      testMount.initialize(makeTestHash("1")),
      std::domain_error,
      "commit 0{39}1 not found");
}

TEST(EdenMount, resolveSymlink) {
  FakeTreeBuilder builder;
  builder.mkdir("src");
  builder.setFile("src/test.c", "testy tests");
  builder.setSymlink("a", "b");
  builder.setSymlink("b", "src/c");
  builder.setSymlink("src/c", "test.c");
  builder.setSymlink("d", "/tmp");
  builder.setSymlink("badlink", "link/to/nowhere");
  builder.setSymlink("link_outside_mount", "../outside_mount");
  builder.setSymlink("loop1", "src/loop2");
  builder.setSymlink("src/loop2", "../loop1");
  builder.setSymlink("src/selfloop", "../src/selfloop");
  builder.setSymlink("src/link_to_dir", "../src");

  builder.mkdir("d1");
  builder.mkdir("d1/d2");
  builder.mkdir("d1/d2/d3");
  builder.setFile("d1/foo.txt", "contents\n");
  builder.setSymlink("d1/d2/d3/somelink", "../../foo.txt");
  builder.setSymlink("d1/d2/d3/anotherlink", "../../../src/test.c");

  TestMount testMount{builder};
  const auto& edenMount = testMount.getEdenMount();

  const auto getInodeBlocking = [edenMount](std::string path) {
    return edenMount->getInodeBlocking(
        RelativePathPiece{folly::StringPiece{path}});
  };

  const auto resolveSymlink = [edenMount](const InodePtr& pInode) {
    return edenMount->resolveSymlink(pInode).get(1s);
  };

  const InodePtr pDir{getInodeBlocking("src")};
  EXPECT_EQ(dtype_t::Dir, pDir->getType());
  const InodePtr pSymlinkA{getInodeBlocking("a")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkA->getType());
  EXPECT_TRUE(pSymlinkA.asFileOrNull() != nullptr);
  const InodePtr pSymlinkB{getInodeBlocking("b")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkB->getType());
  const InodePtr pSymlinkC{getInodeBlocking("src/c")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkC->getType());
  const InodePtr pSymlinkD{getInodeBlocking("d")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkD->getType());
  const InodePtr pSymlinkBadlink{getInodeBlocking("badlink")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkBadlink->getType());
  const InodePtr pSymlinkOutsideMount{getInodeBlocking("link_outside_mount")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkOutsideMount->getType());
  const InodePtr pSymlinkLoop{getInodeBlocking("loop1")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkLoop->getType());
  const InodePtr pLinkToDir{getInodeBlocking("src/link_to_dir")};
  EXPECT_EQ(dtype_t::Symlink, pLinkToDir->getType());

  const InodePtr pTargetFile{getInodeBlocking("src/test.c")};
  EXPECT_EQ(dtype_t::Regular, pTargetFile->getType());
  EXPECT_TRUE(pTargetFile.asFileOrNull() != nullptr);

  EXPECT_TRUE(resolveSymlink(pTargetFile) == pTargetFile);
  EXPECT_TRUE(resolveSymlink(pDir) == pDir);
  EXPECT_TRUE(resolveSymlink(pSymlinkC) == pTargetFile);
  EXPECT_TRUE(resolveSymlink(pSymlinkB) == pTargetFile); // BAD BAD BAD
  EXPECT_TRUE(resolveSymlink(pSymlinkA) == pTargetFile);
  EXPECT_TRUE(resolveSymlink(pLinkToDir) == pDir);

  const InodePtr pFoo{getInodeBlocking("d1/foo.txt")};
  EXPECT_EQ(dtype_t::Regular, pFoo->getType());
  const InodePtr pSymlink2deep{getInodeBlocking("d1/d2/d3/somelink")};
  EXPECT_TRUE(resolveSymlink(pSymlink2deep) == pFoo);
  const InodePtr pSymlink3deep{getInodeBlocking("d1/d2/d3/anotherlink")};
  EXPECT_TRUE(resolveSymlink(pSymlink3deep) == pTargetFile);
  const InodePtr pSelfLoop{getInodeBlocking("src/selfloop")};
  EXPECT_EQ(dtype_t::Symlink, pSelfLoop->getType());

  EXPECT_THROW_ERRNO(resolveSymlink(pSymlinkLoop), ELOOP);
  EXPECT_THROW_ERRNO(resolveSymlink(pSymlinkBadlink), ENOENT);
  EXPECT_THROW_ERRNO(resolveSymlink(pSymlinkOutsideMount), EXDEV);
  EXPECT_THROW_ERRNO(resolveSymlink(pSymlinkD), EPERM);
  EXPECT_THROW_ERRNO(resolveSymlink(pSelfLoop), ELOOP);
}

TEST(EdenMount, resolveSymlinkDelayed) {
  FakeTreeBuilder builder;
  builder.setSymlink("a", "a2");
  builder.setSymlink("a2", "b");
  builder.setFile("b", "contents\n");
  TestMount testMount{builder, /*startReady*/ false};

  // ready "a" and get a INodePtr to it
  builder.setReady("a");
  const auto& edenMount = testMount.getEdenMount();
  const InodePtr pA{
      edenMount->getInodeBlocking(RelativePathPiece{folly::StringPiece{"a"}})};
  EXPECT_EQ(dtype_t::Symlink, pA->getType());

  auto bFuture = edenMount->resolveSymlink(pA);
  EXPECT_FALSE(bFuture.isReady());

  builder.setReady("a2");
  builder.setReady("b");

  const InodePtr pB{
      edenMount->getInodeBlocking(RelativePathPiece{folly::StringPiece{"b"}})};
  EXPECT_EQ(dtype_t::Regular, pB->getType());

  const auto pResolvedB = std::move(bFuture).get(1s);
  EXPECT_TRUE(pResolvedB == pB);
}

TEST(EdenMount, resetParents) {
  TestMount testMount;

  // Prepare two commits
  auto builder1 = FakeTreeBuilder();
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("src/test.c", "testy tests");
  builder1.setFile("doc/readme.txt", "all the words");
  builder1.finalize(testMount.getBackingStore(), true);
  auto commit1 = testMount.getBackingStore()->putCommit("1", builder1);
  commit1->setReady();

  auto builder2 = builder1.clone();
  builder2.replaceFile("src/test.c", "even more testy tests");
  builder2.setFile("src/extra.h", "extra stuff");
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit("2", builder2);
  commit2->setReady();

  // Initialize the TestMount pointing at commit1
  testMount.initialize(makeTestHash("1"));
  const auto& edenMount = testMount.getEdenMount();
  EXPECT_EQ(ParentCommits{makeTestHash("1")}, edenMount->getParentCommits());
  EXPECT_EQ(
      ParentCommits{makeTestHash("1")},
      edenMount->getConfig()->getParentCommits());
  auto latestJournalEntry = edenMount->getJournal().getLatest();
  EXPECT_EQ(makeTestHash("1"), latestJournalEntry->fromHash);
  EXPECT_EQ(makeTestHash("1"), latestJournalEntry->toHash);
  EXPECT_FILE_INODE(testMount.getFileInode("src/test.c"), "testy tests", 0644);
  EXPECT_FALSE(testMount.hasFileAt("src/extra.h"));

  // Reset the TestMount to pointing to commit2
  edenMount->resetParent(makeTestHash("2"));
  // The snapshot ID should be updated, both in memory and on disk
  EXPECT_EQ(ParentCommits{makeTestHash("2")}, edenMount->getParentCommits());
  EXPECT_EQ(
      ParentCommits{makeTestHash("2")},
      edenMount->getConfig()->getParentCommits());
  latestJournalEntry = edenMount->getJournal().getLatest();
  EXPECT_EQ(makeTestHash("1"), latestJournalEntry->fromHash);
  EXPECT_EQ(makeTestHash("2"), latestJournalEntry->toHash);
  // The file contents should not have changed.
  // Even though we are pointing at commit2, the working directory contents
  // still look like commit1.
  EXPECT_FILE_INODE(testMount.getFileInode("src/test.c"), "testy tests", 0644);
  EXPECT_FALSE(testMount.hasFileAt("src/extra.h"));
}

// Tests if last checkout time is getting updated correctly or not.
TEST(EdenMount, testLastCheckoutTime) {
  TestMount testMount;

  auto builder = FakeTreeBuilder();
  builder.setFile("dir/foo.txt", "Fooooo!!");
  builder.finalize(testMount.getBackingStore(), true);
  auto commit = testMount.getBackingStore()->putCommit("1", builder);
  commit->setReady();

  auto sec = std::chrono::seconds{50000};
  auto nsec = std::chrono::nanoseconds{10000};
  auto duration = sec + nsec;
  std::chrono::system_clock::time_point currentTime(
      std::chrono::duration_cast<std::chrono::system_clock::duration>(
          duration));

  testMount.initialize(makeTestHash("1"), currentTime);
  const auto& edenMount = testMount.getEdenMount();
  struct timespec lastCheckoutTime = edenMount->getLastCheckoutTime();

  // Check if EdenMount is updating lastCheckoutTime correctly
  EXPECT_EQ(sec.count(), lastCheckoutTime.tv_sec);
  EXPECT_EQ(nsec.count(), lastCheckoutTime.tv_nsec);

  // Check if FileInode is updating lastCheckoutTime correctly
  auto fileInode = testMount.getFileInode("dir/foo.txt");
  auto stFile = fileInode->getMetadata().timestamps;
  EXPECT_EQ(sec.count(), stFile.atime.toTimespec().tv_sec);
  EXPECT_EQ(nsec.count(), stFile.atime.toTimespec().tv_nsec);
  EXPECT_EQ(sec.count(), stFile.ctime.toTimespec().tv_sec);
  EXPECT_EQ(nsec.count(), stFile.ctime.toTimespec().tv_nsec);
  EXPECT_EQ(sec.count(), stFile.mtime.toTimespec().tv_sec);
  EXPECT_EQ(nsec.count(), stFile.mtime.toTimespec().tv_nsec);

  // Check if TreeInode is updating lastCheckoutTime correctly
  auto treeInode = testMount.getTreeInode("dir");
  auto stDir = treeInode->getMetadata().timestamps;
  EXPECT_EQ(sec.count(), stDir.atime.toTimespec().tv_sec);
  EXPECT_EQ(nsec.count(), stDir.atime.toTimespec().tv_nsec);
  EXPECT_EQ(sec.count(), stDir.ctime.toTimespec().tv_sec);
  EXPECT_EQ(nsec.count(), stDir.ctime.toTimespec().tv_nsec);
  EXPECT_EQ(sec.count(), stDir.mtime.toTimespec().tv_sec);
  EXPECT_EQ(nsec.count(), stDir.mtime.toTimespec().tv_nsec);
}

TEST(EdenMount, testCreatingFileSetsTimestampsToNow) {
  TestMount testMount;

  auto builder = FakeTreeBuilder();
  builder.setFile("initial/file.txt", "was here");
  builder.finalize(testMount.getBackingStore(), true);
  auto commit = testMount.getBackingStore()->putCommit("1", builder);
  commit->setReady();

  auto& clock = testMount.getClock();

  auto lastCheckoutTime = clock.getTimePoint();

  testMount.initialize(makeTestHash("1"), lastCheckoutTime);

  clock.advance(10min);

  auto newFile = testMount.getEdenMount()
                     ->getRootInode()
                     ->create("newfile.txt"_pc, 0660, 0)
                     .get();
  auto fileInode = testMount.getFileInode("newfile.txt");
  auto timestamps = fileInode->getMetadata().timestamps;
  EXPECT_EQ(
      clock.getTimePoint(),
      folly::to<FakeClock::time_point>(timestamps.atime.toTimespec()));
  EXPECT_EQ(
      clock.getTimePoint(),
      folly::to<FakeClock::time_point>(timestamps.ctime.toTimespec()));
  EXPECT_EQ(
      clock.getTimePoint(),
      folly::to<FakeClock::time_point>(timestamps.mtime.toTimespec()));
}

TEST(EdenMount, testCanModifyPermissionsOnFilesAndDirs) {
  TestMount testMount;
  auto builder = FakeTreeBuilder();
  builder.setFile("dir/file.txt", "contents");
  testMount.initialize(builder);

  auto treeInode = testMount.getTreeInode("dir");
  auto fileInode = testMount.getFileInode("dir/file.txt");

  fuse_setattr_in attr{};
  attr.valid = FATTR_MODE;
  int modebits = 07673;
  attr.mode = modebits; // setattr ignores format flags

  auto treeResult = treeInode->setattr(attr).get(0ms);
  EXPECT_EQ(treeInode->getNodeId().get(), treeResult.st.st_ino);
  EXPECT_EQ(S_IFDIR | modebits, treeResult.st.st_mode);

  auto fileResult = fileInode->setattr(attr).get(0ms);
  EXPECT_EQ(fileInode->getNodeId().get(), fileResult.st.st_ino);
  EXPECT_EQ(S_IFREG | modebits, fileResult.st.st_mode);
}

TEST(EdenMount, testCanChownFilesAndDirs) {
  TestMount testMount;
  auto builder = FakeTreeBuilder();
  builder.setFile("dir/file.txt", "contents");
  testMount.initialize(builder);

  auto treeInode = testMount.getTreeInode("dir");
  auto fileInode = testMount.getFileInode("dir/file.txt");

  fuse_setattr_in attr{};
  attr.valid = FATTR_UID | FATTR_GID;
  attr.uid = 23;
  attr.gid = 27;

  auto treeResult = treeInode->setattr(attr).get(0ms);
  EXPECT_EQ(treeInode->getNodeId().get(), treeResult.st.st_ino);
  EXPECT_EQ(attr.uid, treeResult.st.st_uid);
  EXPECT_EQ(attr.gid, treeResult.st.st_gid);

  auto fileResult = fileInode->setattr(attr).get(0ms);
  EXPECT_EQ(fileInode->getNodeId().get(), fileResult.st.st_ino);
  EXPECT_EQ(attr.uid, fileResult.st.st_uid);
  EXPECT_EQ(attr.gid, fileResult.st.st_gid);
}
