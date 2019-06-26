/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/EdenMount.h"

#include <folly/File.h>
#include <folly/Range.h>
#include <folly/ScopeGuard.h>
#include <folly/chrono/Conv.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/futures/FutureSplitter.h>
#include <folly/futures/Promise.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <stdexcept>
#include <utility>

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeFuse.h"
#include "eden/fs/testharness/FakePrivHelper.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace std::chrono_literals;
using std::optional;
using namespace facebook::eden;

namespace {
constexpr folly::Duration kTimeout =
    std::chrono::duration_cast<folly::Duration>(60s);
constexpr folly::Duration kMicroTimeout =
    std::chrono::duration_cast<folly::Duration>(10ms);

template <class Func>
void logAndSwallowExceptions(Func&&);

/**
 * Detect whether an EdenMount object is destructed and deallocated.
 */
class EdenMountDestroyDetector {
 public:
  explicit EdenMountDestroyDetector(TestMount&);

  testing::AssertionResult mountIsAlive();

  testing::AssertionResult mountIsDeleted();

 private:
  std::weak_ptr<EdenMount> weakMount_;
  std::weak_ptr<ServerState> weakServerState_;
  long originalServerStateUseCount_;
};

class MockMountDelegate : public FakePrivHelper::MountDelegate {
 public:
  class MountFailed : public std::exception {};
  class UnmountFailed : public std::exception {};

  FOLLY_NODISCARD folly::Future<folly::File> fuseMount() override;
  FOLLY_NODISCARD folly::Future<folly::Unit> fuseUnmount() override;

  void setMountFuseDevice(folly::File&&);

  void makeMountFail();

  /**
   * Postconditions:
   * - RESULT.getFuture must not be called.
   */
  FOLLY_NODISCARD folly::Promise<folly::File> makeMountPromise();

  /**
   * Postconditions:
   * - fuseUnmount().hasValue() == true
   */
  void makeUnmountSucceed();

  void makeUnmountFail();

  FOLLY_NODISCARD folly::Promise<folly::Unit> makeUnmountPromise();

  FOLLY_NODISCARD bool wasFuseMountEverCalled() const noexcept;

  FOLLY_NODISCARD int getFuseUnmountCallCount() const noexcept;
  FOLLY_NODISCARD bool wasFuseUnmountEverCalled() const noexcept;

 private:
  folly::Future<folly::File> mountFuture_{
      folly::Future<folly::File>::makeEmpty()};
  std::optional<folly::FutureSplitter<folly::Unit>> unmountFuture_;
  int fuseMountCalls_{0};
  int fuseUnmountCalls_{0};
};

class EdenMountShutdownBlocker {
 public:
  /**
   * Mark the EdenMount as 'in use', preventing the Future returned by
   * EdenMount::shutdown() from becoming ready with a value.
   */
  static EdenMountShutdownBlocker preventShutdownFromCompleting(EdenMount&);

  /**
   * Allow the Future returned by EdenMount::shutdown() to becoming ready with a
   * value.
   *
   * When this function returns, there is no guarantee that the Future will be
   * ready. (Something else might prevent the shutdown process from completing.)
   */
  void allowShutdownToComplete();

 private:
  explicit EdenMountShutdownBlocker(InodePtr inode) noexcept;

  EdenMountShutdownBlocker(const EdenMountShutdownBlocker&) = delete;
  EdenMountShutdownBlocker& operator=(const EdenMountShutdownBlocker&) = delete;

  InodePtr inode;
};
} // namespace

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

  const auto getInode = [edenMount](std::string path) {
    return edenMount->getInode(RelativePathPiece{folly::StringPiece{path}})
        .get();
  };

  const auto resolveSymlink = [edenMount](const InodePtr& pInode) {
    return edenMount->resolveSymlink(pInode).get(1s);
  };

  const InodePtr pDir{getInode("src")};
  EXPECT_EQ(dtype_t::Dir, pDir->getType());
  const InodePtr pSymlinkA{getInode("a")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkA->getType());
  EXPECT_TRUE(pSymlinkA.asFileOrNull() != nullptr);
  const InodePtr pSymlinkB{getInode("b")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkB->getType());
  const InodePtr pSymlinkC{getInode("src/c")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkC->getType());
  const InodePtr pSymlinkD{getInode("d")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkD->getType());
  const InodePtr pSymlinkBadlink{getInode("badlink")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkBadlink->getType());
  const InodePtr pSymlinkOutsideMount{getInode("link_outside_mount")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkOutsideMount->getType());
  const InodePtr pSymlinkLoop{getInode("loop1")};
  EXPECT_EQ(dtype_t::Symlink, pSymlinkLoop->getType());
  const InodePtr pLinkToDir{getInode("src/link_to_dir")};
  EXPECT_EQ(dtype_t::Symlink, pLinkToDir->getType());

  const InodePtr pTargetFile{getInode("src/test.c")};
  EXPECT_EQ(dtype_t::Regular, pTargetFile->getType());
  EXPECT_TRUE(pTargetFile.asFileOrNull() != nullptr);

  EXPECT_TRUE(resolveSymlink(pTargetFile) == pTargetFile);
  EXPECT_TRUE(resolveSymlink(pDir) == pDir);
  EXPECT_TRUE(resolveSymlink(pSymlinkC) == pTargetFile);
  EXPECT_TRUE(resolveSymlink(pSymlinkB) == pTargetFile); // BAD BAD BAD
  EXPECT_TRUE(resolveSymlink(pSymlinkA) == pTargetFile);
  EXPECT_TRUE(resolveSymlink(pLinkToDir) == pDir);

  const InodePtr pFoo{getInode("d1/foo.txt")};
  EXPECT_EQ(dtype_t::Regular, pFoo->getType());
  const InodePtr pSymlink2deep{getInode("d1/d2/d3/somelink")};
  EXPECT_TRUE(resolveSymlink(pSymlink2deep) == pFoo);
  const InodePtr pSymlink3deep{getInode("d1/d2/d3/anotherlink")};
  EXPECT_TRUE(resolveSymlink(pSymlink3deep) == pTargetFile);
  const InodePtr pSelfLoop{getInode("src/selfloop")};
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
      edenMount->getInode(RelativePathPiece{folly::StringPiece{"a"}}).get()};
  EXPECT_EQ(dtype_t::Symlink, pA->getType());

  auto bFuture = edenMount->resolveSymlink(pA);
  EXPECT_FALSE(bFuture.isReady());

  builder.setReady("a2");
  builder.setReady("b");

  const InodePtr pB{
      edenMount->getInode(RelativePathPiece{folly::StringPiece{"b"}}).get()};
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
  ASSERT_TRUE(latestJournalEntry);
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
  ASSERT_TRUE(latestJournalEntry);
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

TEST(EdenMount, ensureDirectoryExists) {
  auto builder = FakeTreeBuilder{};
  builder.mkdir("sub/foo/bar");
  builder.setFile("sub/file.txt", "");
  TestMount testMount{builder};
  auto edenMount = testMount.getEdenMount();

  edenMount->ensureDirectoryExists("sub/foo/bar"_relpath).get(0ms);
  EXPECT_NE(nullptr, testMount.getTreeInode("sub/foo/bar"));

  edenMount->ensureDirectoryExists("sub/other/stuff/here"_relpath).get(0ms);
  EXPECT_NE(nullptr, testMount.getTreeInode("sub/other/stuff/here"));

  auto f1 =
      edenMount->ensureDirectoryExists("sub/file.txt/baz"_relpath).wait(0ms);
  EXPECT_TRUE(f1.isReady());
  EXPECT_THROW(std::move(f1).get(0ms), std::system_error);

  auto f2 = edenMount->ensureDirectoryExists("sub/file.txt"_relpath).wait(0ms);
  EXPECT_TRUE(f2.isReady());
  EXPECT_THROW(std::move(f2).get(0ms), std::system_error);
}

TEST(EdenMount, concurrentDeepEnsureDirectoryExists) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto edenMount = testMount.getEdenMount();

  auto dirPath = "foo/bar/baz/this/should/be/very/long"_relpath;

  constexpr unsigned kThreadCount = 10;

  std::vector<std::thread> threads;
  threads.reserve(kThreadCount);
  std::vector<folly::Baton<>> batons{kThreadCount};

  for (unsigned i = 0; i < kThreadCount; ++i) {
    threads.emplace_back([&, i] {
      batons[i].wait();
      try {
        edenMount->ensureDirectoryExists(dirPath).get(0ms);
      } catch (std::exception& e) {
        printf("ensureDirectoryExists failed: %s\n", e.what());
        throw;
      }
    });
  }

  for (auto& baton : batons) {
    baton.post();
  }

  for (auto& thread : threads) {
    thread.join();
  }

  EXPECT_NE(nullptr, testMount.getTreeInode(dirPath));
}

TEST(EdenMount, setOwnerChangesTakeEffect) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "contents");
  TestMount testMount{builder};
  auto edenMount = testMount.getEdenMount();

  uid_t uid = 1024;
  gid_t gid = 2048;
  edenMount->setOwner(uid, gid);

  auto fileInode = testMount.getFileInode("dir/file.txt");
  auto attr = fileInode->getattr().get(0ms);
  EXPECT_EQ(attr.st.st_uid, uid);
  EXPECT_EQ(attr.st.st_gid, gid);
}

class ChownTest : public ::testing::Test {
 protected:
  const uid_t uid = 1024;
  const gid_t gid = 2048;

  void SetUp() override {
    builder_.setFile("file.txt", "contents");
    testMount_ = std::make_unique<TestMount>(builder_);
    edenMount_ = testMount_->getEdenMount();
    fuse_ = std::make_shared<FakeFuse>();
    testMount_->startFuseAndWait(fuse_);
  }

  InodeNumber load() {
    auto file = edenMount_->getInode("file.txt"_relpath).get();
    // Load the file into the inode map
    file->incFuseRefcount();
    file->getNodeId();
    return file->getNodeId();
  }

  void expectChownSucceeded() {
    auto attr = testMount_->getFileInode("file.txt")->getattr().get(0ms);
    EXPECT_EQ(attr.st.st_uid, uid);
    EXPECT_EQ(attr.st.st_gid, gid);
  }

  bool invalidatedFileInode(InodeNumber fileIno) {
    auto responses = fuse_->getAllResponses();
    bool invalidatedInode = false;
    for (const auto& response : responses) {
      EXPECT_EQ(response.header.error, FUSE_NOTIFY_INVAL_INODE);
      auto out = reinterpret_cast<const fuse_notify_inval_inode_out*>(
          response.body.data());
      if (out->ino == fileIno.get()) {
        invalidatedInode = true;
      }
    }
    return invalidatedInode;
  }

  FakeTreeBuilder builder_;
  std::unique_ptr<TestMount> testMount_;
  std::shared_ptr<FakeFuse> fuse_;
  std::shared_ptr<EdenMount> edenMount_;
};

TEST_F(ChownTest, UnloadedInodeWithZeroRefCount) {
  auto inodeMap = edenMount_->getInodeMap();

  auto fileIno = load();
  EXPECT_TRUE(inodeMap->lookupInode(fileIno).get());
  // now unload it with a zero ref count
  inodeMap->decFuseRefcount(fileIno, 1);
  edenMount_->getRootInode()->unloadChildrenNow();

  auto chownFuture = edenMount_->chown(uid, gid);
  EXPECT_FALSE(invalidatedFileInode(fileIno));
  std::move(chownFuture).get(10s);

  expectChownSucceeded();
}

TEST_F(ChownTest, UnloadedInodeWithPositiveRefCount) {
  auto inodeMap = edenMount_->getInodeMap();

  auto fileIno = load();
  EXPECT_TRUE(inodeMap->lookupInode(fileIno).get());
  // now unload it with a positive ref count
  edenMount_->getRootInode()->unloadChildrenNow();

  auto chownFuture = edenMount_->chown(uid, gid);
  EXPECT_TRUE(invalidatedFileInode(fileIno));
  std::move(chownFuture).get(10s);

  expectChownSucceeded();
}

TEST_F(ChownTest, LoadedInode) {
  auto inodeMap = edenMount_->getInodeMap();

  auto fileIno = load();
  EXPECT_TRUE(inodeMap->lookupInode(fileIno).get());
  edenMount_->getRootInode()->unloadChildrenNow();

  auto chownFuture = edenMount_->chown(uid, gid);
  EXPECT_TRUE(invalidatedFileInode(fileIno));
  std::move(chownFuture).get(10s);

  expectChownSucceeded();
}

TEST(EdenMount, destroyDeletesObjectAfterInProgressShutdownCompletes) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto mountDestroyDetector = EdenMountDestroyDetector{testMount};
  std::shared_ptr<EdenMount>& mount = testMount.getEdenMount();

  auto shutdownBlocker =
      EdenMountShutdownBlocker::preventShutdownFromCompleting(*mount);

  auto shutdownFuture =
      mount->shutdown(/*doTakeover=*/false, /*allowFuseNotStarted=*/true);
  mount.reset();
  EXPECT_TRUE(mountDestroyDetector.mountIsAlive())
      << "EdenMount object should be alive during EdenMount::shutdown";

  shutdownBlocker.allowShutdownToComplete();
  std::move(shutdownFuture).get(kTimeout);
  EXPECT_TRUE(mountDestroyDetector.mountIsDeleted())
      << "EdenMount object should be deleted during EdenMount::shutdown";
}

TEST(
    EdenMount,
    destroyDeletesObjectIfInProgressFuseConnectionIsCancelledDuringShutdown) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto mountDestroyDetector = EdenMountDestroyDetector{testMount};
  std::shared_ptr<EdenMount>& mount = testMount.getEdenMount();

  auto shutdownBlocker =
      EdenMountShutdownBlocker::preventShutdownFromCompleting(*mount);

  auto fuse = std::make_shared<FakeFuse>();
  testMount.registerFakeFuse(fuse);
  auto startFuseFuture = mount->startFuse();

  mount.reset();
  fuse->close();

  // TODO(strager): Ensure mount is only destroyed after startFuseFuture is
  // ready. (I.e. if FuseChannel::initialize is in progress,
  // EdenMount::~EdenMount should not be called.)

  logAndSwallowExceptions([&] { std::move(startFuseFuture).get(kTimeout); });
  EXPECT_TRUE(mountDestroyDetector.mountIsAlive())
      << "Eden mount should be alive during EdenMount::destroy despite failure in startFuse";

  shutdownBlocker.allowShutdownToComplete();
  EXPECT_TRUE(mountDestroyDetector.mountIsDeleted());
}

TEST(EdenMount, unmountSucceedsIfNeverMounted) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  mount.unmount().get(kTimeout);
  EXPECT_FALSE(mountDelegate->wasFuseUnmountEverCalled())
      << "unmount should not call fuseUnmount";
}

TEST(EdenMount, unmountDoesNothingIfPriorMountFailed) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);
  mountDelegate->makeMountFail();
  mountDelegate->makeUnmountFail();

  ASSERT_THROW(
      { mount.startFuse().get(kTimeout); }, MockMountDelegate::MountFailed);
  EXPECT_NO_THROW({ mount.unmount().get(kTimeout); });
  EXPECT_FALSE(mountDelegate->wasFuseUnmountEverCalled())
      << "unmount should not call fuseUnmount";
}

TEST(EdenMount, unmountIsIdempotent) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);
  auto fuse = std::make_shared<FakeFuse>();
  mountDelegate->setMountFuseDevice(fuse->start());
  mountDelegate->makeUnmountSucceed();

  auto startFuseFuture = mount.startFuse();
  fuse->sendInitRequest();
  fuse->recvResponse();
  std::move(startFuseFuture)
      .within(kTimeout)
      .getVia(testMount.getServerExecutor().get());
  SCOPE_EXIT {
    fuse->close();
    mount.getFuseCompletionFuture().within(kTimeout).getVia(
        testMount.getServerExecutor().get());
  };

  mount.unmount().get(kTimeout);
  mount.unmount().get(kTimeout);
  EXPECT_EQ(mountDelegate->getFuseUnmountCallCount(), 1)
      << "fuseUnmount should be called only once despite multiple calls to unmount";
}

TEST(EdenMount, concurrentUnmountCallsWaitForExactlyOneFuseUnmount) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  auto unmountPromise = mountDelegate->makeUnmountPromise();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto fuse = std::make_shared<FakeFuse>();
  mountDelegate->setMountFuseDevice(fuse->start());

  auto startFuseFuture = mount.startFuse();
  fuse->sendInitRequest();
  fuse->recvResponse();
  std::move(startFuseFuture)
      .within(kTimeout)
      .getVia(testMount.getServerExecutor().get());
  SCOPE_EXIT {
    fuse->close();
    mount.getFuseCompletionFuture().within(kTimeout).getVia(
        testMount.getServerExecutor().get());
  };

  auto unmountFuture1 = mount.unmount();
  auto unmountFuture2 = mount.unmount();
  EXPECT_FALSE(unmountFuture1.isReady())
      << "unmount should not finish before fuseUnmount returns";
  EXPECT_FALSE(unmountFuture2.isReady())
      << "unmount should not finish before fuseUnmount returns";

  unmountPromise.setValue();

  std::move(unmountFuture1).get(kTimeout);
  std::move(unmountFuture2).get(kTimeout);

  EXPECT_EQ(mountDelegate->getFuseUnmountCallCount(), 1)
      << "fuseUnmount should be called only once despite multiple calls to unmount";
}

TEST(EdenMount, unmountUnmountsIfMounted) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto fuse = std::make_shared<FakeFuse>();
  auto mountDelegate =
      std::make_shared<FakeFuseMountDelegate>(mount.getPath(), fuse);
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto startFuseFuture = mount.startFuse();
  fuse->sendInitRequest();
  fuse->recvResponse();
  std::move(startFuseFuture)
      .within(kTimeout)
      .getVia(testMount.getServerExecutor().get());

  mount.unmount().get(kTimeout);
  SCOPE_EXIT {
    mount.getFuseCompletionFuture().within(kTimeout).getVia(
        testMount.getServerExecutor().get());
  };

  EXPECT_TRUE(mountDelegate->wasFuseUnmountEverCalled())
      << "unmount should call fuseUnmount";
}

TEST(EdenMount, unmountUnmountsIfTookOver) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto fuse = std::make_shared<FakeFuse>();
  auto mountDelegate =
      std::make_shared<FakeFuseMountDelegate>(mount.getPath(), fuse);
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  mount.takeoverFuse(FuseChannelData{fuse->start(), {}});

  mount.unmount().get(kTimeout);
  SCOPE_EXIT {
    mount.getFuseCompletionFuture().within(kTimeout).getVia(
        testMount.getServerExecutor().get());
  };
  EXPECT_TRUE(mountDelegate->wasFuseUnmountEverCalled())
      << "unmount should call fuseUnmount";
}

TEST(EdenMount, cancelledMountDoesNotUnmountIfMountingFails) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  auto mountPromise = mountDelegate->makeMountPromise();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto startFuseFuture = mount.startFuse();
  auto unmountFuture = mount.unmount();

  auto unmountCallCountBeforeMountFails =
      mountDelegate->getFuseUnmountCallCount();
  mountPromise.setException(MockMountDelegate::MountFailed{});

  EXPECT_THROW(
      std::move(startFuseFuture).get(kTimeout), MockMountDelegate::MountFailed);
  EXPECT_NO_THROW({ std::move(unmountFuture).get(kTimeout); });
  EXPECT_EQ(
      mountDelegate->getFuseUnmountCallCount(),
      unmountCallCountBeforeMountFails)
      << "fuseUnmount should not be called after fuseMount fails";
}

TEST(EdenMount, unmountCancelsInProgressMount) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  auto mountPromise = mountDelegate->makeMountPromise();
  mountDelegate->makeUnmountSucceed();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto startFuseFuture = mount.startFuse();
  auto unmountFuture = mount.unmount();
  SCOPE_EXIT {
    std::move(unmountFuture).get(kTimeout);
  };

  auto unmountCallCountBeforeMountCompletes =
      mountDelegate->getFuseUnmountCallCount();
  auto fuse = std::make_shared<FakeFuse>();
  mountPromise.setValue(fuse->start());

  EXPECT_THROW(
      std::move(startFuseFuture).within(kTimeout).get(),
      FuseDeviceUnmountedDuringInitialization);
  EXPECT_EQ(
      mountDelegate->getFuseUnmountCallCount(),
      unmountCallCountBeforeMountCompletes + 1)
      << "fuseUnmount should be called exactly once after fuseMount completes";
}

TEST(EdenMount, cancelledMountWaitsForUnmountBeforeCompleting) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  auto mountPromise = mountDelegate->makeMountPromise();
  auto unmountPromise = mountDelegate->makeUnmountPromise();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto startFuseFuture = mount.startFuse();
  auto unmountFuture = mount.unmount();
  SCOPE_EXIT {
    std::move(unmountFuture).get(kTimeout);
  };

  auto fuse = std::make_shared<FakeFuse>();
  mountPromise.setValue(fuse->start());

  EXPECT_FALSE(startFuseFuture.wait(kMicroTimeout).isReady())
      << "startFuse should wait until fuseUnmount completes";
  unmountPromise.setValue();
  EXPECT_TRUE(startFuseFuture.wait(kTimeout).isReady())
      << "startFuse should complete after fuseUnmount completes";
}

TEST(EdenMount, unmountWaitsForInProgressMountBeforeUnmounting) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  auto mountPromise = mountDelegate->makeMountPromise();
  mountDelegate->makeUnmountSucceed();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto startFuseFuture = mount.startFuse();
  auto unmountFuture = mount.unmount();

  EXPECT_FALSE(mountDelegate->wasFuseUnmountEverCalled())
      << "unmount should not call fuseUnmount until fuseMount completes";
  ASSERT_FALSE(unmountFuture.wait(kMicroTimeout).isReady())
      << "unmount should not finish until fuseMount completes";

  auto fuse = std::make_shared<FakeFuse>();
  mountPromise.setValue(fuse->start());

  try {
    std::move(startFuseFuture).within(kTimeout).get();
  } catch (FuseDeviceUnmountedDuringInitialization&) {
  }
  std::move(unmountFuture).get(kTimeout);
  EXPECT_TRUE(mountDelegate->wasFuseUnmountEverCalled())
      << "fuseUnmount should be called after fuseMount completes";
}

TEST(EdenMount, unmountingDuringFuseHandshakeCancelsStart) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto fuse = std::make_shared<FakeFuse>();
  auto mountDelegate =
      std::make_shared<FakeFuseMountDelegate>(mount.getPath(), fuse);
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto startFuseFuture = mount.startFuse();
  ASSERT_FALSE(startFuseFuture.wait(kMicroTimeout).isReady())
      << "startFuse should not finish before FUSE handshake";

  auto unmountFuture = mount.unmount();
  EXPECT_THROW(
      std::move(startFuseFuture).get(kTimeout),
      FuseDeviceUnmountedDuringInitialization)
      << "unmount should cancel startFuse";

  std::move(unmountFuture).get(kTimeout);
  EXPECT_TRUE(mountDelegate->wasFuseUnmountEverCalled())
      << "unmount should call fuseUnmount";
}

TEST(EdenMount, startingFuseFailsImmediatelyIfUnmountWasEverCalled) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  mount.unmount().within(kTimeout).get();

  EXPECT_THROW(mount.startFuse().get(kTimeout), EdenMountCancelled);
  EXPECT_FALSE(mountDelegate->wasFuseMountEverCalled())
      << "startFuse should fail and not call fuseMount";
}

TEST(EdenMount, takeoverFuseFailsIfUnmountWasEverCalled) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  mount.unmount().within(kTimeout).get();
  auto fuse = std::make_shared<FakeFuse>();
  EXPECT_THROW(
      {
        mount.takeoverFuse(FuseChannelData{fuse->start(), {}});
      },
      EdenMountCancelled);
}

TEST(EdenMountState, mountIsUninitializedAfterConstruction) {
  auto testMount = TestMount{};
  auto builder = FakeTreeBuilder{};
  testMount.createMountWithoutInitializing(builder);
  EXPECT_EQ(
      testMount.getEdenMount()->getState(), EdenMount::State::UNINITIALIZED);
}

TEST(EdenMountState, mountIsInitializedAfterInitializationCompletes) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  EXPECT_EQ(
      testMount.getEdenMount()->getState(), EdenMount::State::INITIALIZED);
}

TEST(EdenMountState, mountIsStartingBeforeMountCompletes) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  auto mountPromise = mountDelegate->makeMountPromise();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto startFuseFuture = mount.startFuse();
  SCOPE_EXIT {
    mountPromise.setException(MockMountDelegate::MountFailed{});
    logAndSwallowExceptions([&] { std::move(startFuseFuture).get(kTimeout); });
  };
  EXPECT_FALSE(startFuseFuture.wait(kMicroTimeout).isReady())
      << "startFuse should not finish before FUSE mounting completes";
  EXPECT_EQ(mount.getState(), EdenMount::State::STARTING);
}

TEST(EdenMountState, mountIsStartingBeforeFuseInitializationCompletes) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto fuse = std::make_shared<FakeFuse>();
  testMount.registerFakeFuse(fuse);

  auto startFuseFuture = mount.startFuse();
  SCOPE_EXIT {
    fuse->close();
    logAndSwallowExceptions([&] { std::move(startFuseFuture).get(kTimeout); });
  };
  EXPECT_FALSE(startFuseFuture.wait(kMicroTimeout).isReady())
      << "startFuse should not finish before FUSE initialization completes";
  EXPECT_EQ(mount.getState(), EdenMount::State::STARTING);
}

TEST(EdenMountState, mountIsRunningAfterFuseInitializationCompletes) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto fuse = std::make_shared<FakeFuse>();
  testMount.startFuseAndWait(fuse);
  EXPECT_EQ(testMount.getEdenMount()->getState(), EdenMount::State::RUNNING);
}

TEST(EdenMountState, newMountIsRunningAndOldMountIsShutDownAfterFuseTakeover) {
  auto oldTestMount = TestMount{FakeTreeBuilder{}};
  auto& oldMount = *oldTestMount.getEdenMount();
  auto newTestMount = TestMount{FakeTreeBuilder{}};
  auto& newMount = *newTestMount.getEdenMount();

  auto fuse = std::make_shared<FakeFuse>();
  oldTestMount.startFuseAndWait(fuse);

  oldMount.getFuseChannel()->takeoverStop();

  TakeoverData::MountInfo takeoverData =
      oldMount.getFuseCompletionFuture().within(kTimeout).getVia(
          oldTestMount.getServerExecutor().get());
  oldMount.shutdown(/*doTakeover=*/true).get(kTimeout);
  newMount.takeoverFuse(FuseChannelData{std::move(takeoverData.fuseFD), {}});

  EXPECT_EQ(oldMount.getState(), EdenMount::State::SHUT_DOWN);
  EXPECT_EQ(newMount.getState(), EdenMount::State::RUNNING);
}

TEST(EdenMountState, mountIsFuseErrorAfterMountFails) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);
  mountDelegate->makeMountFail();

  logAndSwallowExceptions([&] { mount.startFuse().get(kTimeout); });
  EXPECT_EQ(mount.getState(), EdenMount::State::FUSE_ERROR);
}

TEST(EdenMountState, mountIsFuseErrorAfterFuseInitializationFails) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto fuse = std::make_shared<FakeFuse>();
  testMount.registerFakeFuse(fuse);

  auto startFuseFuture = mount.startFuse();
  EXPECT_FALSE(startFuseFuture.wait(kMicroTimeout).isReady())
      << "startFuse should not finish before FUSE mounting completes";

  fuse->close();
  logAndSwallowExceptions([&] { std::move(startFuseFuture).get(kTimeout); });

  EXPECT_EQ(testMount.getEdenMount()->getState(), EdenMount::State::FUSE_ERROR);
}

TEST(EdenMountState, mountIsShuttingDownWhileInodeIsReferencedDuringShutdown) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto* executor = testMount.getServerExecutor().get();

  auto inode = mount.getInodeMap()->getRootInode();

  auto shutdownFutures = folly::FutureSplitter<SerializedInodeMap>{
      mount.shutdown(/*doTakeover=*/false, /*allowFuseNotStarted=*/true)
          .via(executor)};
  SCOPE_EXIT {
    inode.reset();
    shutdownFutures.getFuture().within(kTimeout).getVia(executor);
  };
  EXPECT_THROW(
      shutdownFutures.getFuture().within(kMicroTimeout).getVia(executor),
      folly::FutureTimeout)
      << "shutdown should not finish while inode is referenced";
  EXPECT_EQ(mount.getState(), EdenMount::State::SHUTTING_DOWN);
}

TEST(EdenMountState, mountIsShutDownAfterShutdownCompletes) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  mount.shutdown(/*doTakeover=*/false, /*allowFuseNotStarted=*/true)
      .get(kTimeout);
  EXPECT_EQ(testMount.getEdenMount()->getState(), EdenMount::State::SHUT_DOWN);
}

TEST(EdenMountState, mountIsDestroyingWhileInodeIsReferencedDuringDestroy) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDestroyDetector = EdenMountDestroyDetector{testMount};

  auto inode = mount.getInodeMap()->getRootInode();
  testMount.getEdenMount().reset();
  ASSERT_TRUE(mountDestroyDetector.mountIsAlive())
      << "Eden mount should be alive during EdenMount::destroy";
  EXPECT_EQ(mount.getState(), EdenMount::State::DESTROYING);
}

namespace {
template <class Func>
void logAndSwallowExceptions(Func&& function) {
  try {
    std::forward<Func>(function)();
  } catch (const std::exception& e) {
    XLOG(ERR) << "Ignoring exception: " << e.what();
  }
}

EdenMountDestroyDetector::EdenMountDestroyDetector(TestMount& testMount)
    : weakMount_{testMount.getEdenMount()},
      weakServerState_{testMount.getServerState()},
      originalServerStateUseCount_{weakServerState_.use_count()} {}

testing::AssertionResult EdenMountDestroyDetector::mountIsAlive() {
  auto serverStateUseCount = weakServerState_.use_count();
  if (serverStateUseCount > originalServerStateUseCount_) {
    return testing::AssertionFailure()
        << "Current ServerState shared_ptr use count: " << serverStateUseCount
        << "\nOriginal ServerState shared_ptr use count: "
        << originalServerStateUseCount_;
  }
  return testing::AssertionSuccess();
}

testing::AssertionResult EdenMountDestroyDetector::mountIsDeleted() {
  if (!weakMount_.expired()) {
    return testing::AssertionFailure() << "EdenMount shared_ptr is not expired";
  }
  auto serverStateUseCount = weakServerState_.use_count();
  if (serverStateUseCount >= originalServerStateUseCount_) {
    return testing::AssertionFailure()
        << "Current ServerState shared_ptr use count: " << serverStateUseCount
        << "\nOriginal ServerState shared_ptr use count: "
        << originalServerStateUseCount_;
  }
  return testing::AssertionSuccess();
}

folly::Future<folly::File> MockMountDelegate::fuseMount() {
  fuseMountCalls_ += 1;
  if (mountFuture_.valid()) {
    return std::move(mountFuture_);
  } else {
    return folly::makeFuture<folly::File>(MountFailed{});
  }
}

folly::Future<folly::Unit> MockMountDelegate::fuseUnmount() {
  fuseUnmountCalls_ += 1;
  if (unmountFuture_) {
    return unmountFuture_->getFuture();
  } else {
    return folly::makeFuture<folly::Unit>(UnmountFailed{});
  }
}

void MockMountDelegate::setMountFuseDevice(folly::File&& fuseDevice) {
  CHECK(!mountFuture_.valid())
      << __func__ << " unexpectedly called more than once";
  CHECK(!wasFuseMountEverCalled())
      << __func__ << " unexpectedly called after fuseMount was called";
  mountFuture_ = folly::makeFuture(std::move(fuseDevice));
}

void MockMountDelegate::makeMountFail() {
  CHECK(!mountFuture_.valid())
      << __func__ << " unexpectedly called more than once";
  CHECK(!wasFuseMountEverCalled())
      << __func__ << " unexpectedly called after fuseMount was called";
  mountFuture_ = folly::makeFuture<folly::File>(MountFailed{});
}

folly::Promise<folly::File> MockMountDelegate::makeMountPromise() {
  CHECK(!mountFuture_.valid())
      << __func__ << " unexpectedly called more than once";
  auto promise = folly::Promise<folly::File>{};
  mountFuture_ = promise.getFuture();
  return promise;
}

void MockMountDelegate::makeUnmountSucceed() {
  CHECK(!unmountFuture_) << __func__ << " unexpectedly called more than once";
  CHECK(!wasFuseUnmountEverCalled())
      << __func__ << " unexpectedly called after fuseUnmount was called";
  unmountFuture_.emplace(folly::makeFuture());
}

void MockMountDelegate::makeUnmountFail() {
  CHECK(!unmountFuture_) << __func__ << " unexpectedly called more than once";
  CHECK(!wasFuseUnmountEverCalled())
      << __func__ << " unexpectedly called after fuseUnmount was called";
  unmountFuture_.emplace(folly::makeFuture<folly::Unit>(UnmountFailed{}));
}

folly::Promise<folly::Unit> MockMountDelegate::makeUnmountPromise() {
  CHECK(!unmountFuture_) << __func__ << " unexpectedly called more than once";
  CHECK(!wasFuseUnmountEverCalled())
      << __func__ << " unexpectedly called after fuseUnmount was called";
  auto promise = folly::Promise<folly::Unit>{};
  unmountFuture_.emplace(promise.getFuture());
  return promise;
}

bool MockMountDelegate::wasFuseMountEverCalled() const noexcept {
  return fuseMountCalls_ > 0;
}

int MockMountDelegate::getFuseUnmountCallCount() const noexcept {
  return fuseUnmountCalls_;
}

bool MockMountDelegate::wasFuseUnmountEverCalled() const noexcept {
  return fuseUnmountCalls_ > 0;
}

EdenMountShutdownBlocker
EdenMountShutdownBlocker::preventShutdownFromCompleting(EdenMount& mount) {
  auto inode = mount.getInodeMap()->getRootInode();
  CHECK(inode);
  return EdenMountShutdownBlocker{std::move(inode)};
}

void EdenMountShutdownBlocker::allowShutdownToComplete() {
  inode.reset();
}

EdenMountShutdownBlocker::EdenMountShutdownBlocker(InodePtr inode) noexcept
    : inode{std::move(inode)} {}
} // namespace
