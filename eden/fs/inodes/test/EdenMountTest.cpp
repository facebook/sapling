/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/EdenMount.h"

#include <folly/File.h>
#include <folly/Range.h>
#include <folly/ScopeGuard.h>
#include <folly/chrono/Conv.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/futures/FutureSplitter.h>
#include <folly/futures/Promise.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <stdexcept>
#include <utility>

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeFuse.h"
#include "eden/fs/testharness/FakePrivHelper.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace std::chrono_literals;
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
      testMount.initialize(RootId("1")),
      std::domain_error,
      "commit 1 not found");
}

TEST(EdenMount, getTreeOrTreeEntry) {
  FakeTreeBuilder builder;
  builder.mkdir("src");
  builder.setFile("src/test.c", "testy tests");
  builder.mkdir("bar");
  builder.mkdir("bar/baz");
  builder.setFile("bar/baz/test.c", "this is a test");

  TestMount testMount{builder};
  const auto& edenMount = testMount.getEdenMount();

  {
    auto variant = edenMount
                       ->getTreeOrTreeEntry(
                           ""_relpath, ObjectFetchContext::getNullContext())
                       .get(0ms);
    auto& tree = std::get<std::shared_ptr<const Tree>>(variant);
    EXPECT_EQ(*tree, *testMount.getRootTree());
  }

  {
    auto variant =
        edenMount
            ->getTreeOrTreeEntry(
                "src/test.c"_relpath, ObjectFetchContext::getNullContext())
            .get(0ms);
    auto& treeEntry = std::get<TreeEntry>(variant);
    EXPECT_EQ(treeEntry.getType(), TreeEntryType::REGULAR_FILE);
    auto& storedBlob = builder.getStoredBlob("src/test.c"_relpath)->get();
    EXPECT_EQ(treeEntry.getHash(), storedBlob.getHash());
  }

  {
    auto variant =
        edenMount
            ->getTreeOrTreeEntry(
                "bar/baz"_relpath, ObjectFetchContext::getNullContext())
            .get(0ms);
    auto& tree = std::get<std::shared_ptr<const Tree>>(variant);
    auto& storedTree = builder.getStoredTree("bar/baz"_relpath)->get();
    EXPECT_EQ(tree->getHash(), storedTree.getHash());
  }

  {
    auto fut = edenMount->getTreeOrTreeEntry(
        "not/present"_relpath, ObjectFetchContext::getNullContext());
    EXPECT_THROW_ERRNO(std::move(fut).get(0ms), ENOENT);
  }

  {
    auto fut = edenMount->getTreeOrTreeEntry(
        "bar/baz/test.c/foo"_relpath, ObjectFetchContext::getNullContext());
    EXPECT_THROW_ERRNO(std::move(fut).get(0ms), ENOTDIR);
  }
}

TEST(EdenMount, canonicalizePathFromTree) {
  FakeTreeBuilder builder;
  builder.mkdir("src");
  builder.setFile("src/test.c", "testy tests");
  builder.mkdir("bar");
  builder.mkdir("bar/baz");
  builder.setFile("bar/baz/TEST.c", "this is a test");

  TestMount testMount{builder};
  const auto& edenMount = testMount.getEdenMount();

  {
    auto path =
        edenMount
            ->canonicalizePathFromTree(
                "bar/baz/TEST.c"_relpath, ObjectFetchContext::getNullContext())
            .get(0ms);
    EXPECT_EQ(path, "bar/baz/TEST.c"_relpath);
  }

  if (folly::kIsWindows) {
    {
      auto path =
          edenMount
              ->canonicalizePathFromTree(
                  "SRC/TEST.c"_relpath, ObjectFetchContext::getNullContext())
              .get(0ms);
      EXPECT_EQ(path, "src/test.c"_relpath);
    }

    {
      auto path =
          edenMount
              ->canonicalizePathFromTree(
                  "bar/BAZ"_relpath, ObjectFetchContext::getNullContext())
              .get(0ms);
      EXPECT_EQ(path, "bar/baz"_relpath);
    }

    {
      auto path = edenMount
                      ->canonicalizePathFromTree(
                          "bar/BAZ/test.c"_relpath,
                          ObjectFetchContext::getNullContext())
                      .get(0ms);
      EXPECT_EQ(path, "bar/baz/TEST.c"_relpath);
    }
  }
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
  testMount.initialize(RootId("1"));
  const auto& edenMount = testMount.getEdenMount();
  EXPECT_EQ(RootId("1"), edenMount->getCheckedOutRootId());
  EXPECT_EQ(
      ParentCommit(ParentCommit::WorkingCopyParentAndCheckedOutRevision{
          RootId("1"), RootId("1")}),
      edenMount->getCheckoutConfig()->getParentCommit());
  auto latestJournalEntry = edenMount->getJournal().getLatest();
  ASSERT_TRUE(latestJournalEntry);
  EXPECT_EQ(RootId("1"), latestJournalEntry->fromHash);
  EXPECT_EQ(RootId("1"), latestJournalEntry->toHash);
  EXPECT_FILE_INODE(testMount.getFileInode("src/test.c"), "testy tests", 0644);
  EXPECT_FALSE(testMount.hasFileAt("src/extra.h"));

  // Reset the TestMount to pointing to commit2
  edenMount->resetParent(RootId("2"));
  // The snapshot ID should be updated, both in memory and on disk
  EXPECT_EQ(RootId("1"), edenMount->getCheckedOutRootId());
  EXPECT_EQ(
      ParentCommit(ParentCommit::WorkingCopyParentAndCheckedOutRevision{
          RootId("2"), RootId("1")}),
      edenMount->getCheckoutConfig()->getParentCommit());
  latestJournalEntry = edenMount->getJournal().getLatest();
  ASSERT_TRUE(latestJournalEntry);
  EXPECT_EQ(RootId("1"), latestJournalEntry->fromHash);
  EXPECT_EQ(RootId("2"), latestJournalEntry->toHash);
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

  testMount.initialize(RootId("1"), currentTime);
  const auto& edenMount = testMount.getEdenMount();
  struct timespec lastCheckoutTime =
      edenMount->getLastCheckoutTime().toTimespec();

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

  testMount.initialize(RootId("1"), lastCheckoutTime);

  clock.advance(10min);

  auto newFile = testMount.getEdenMount()->getRootInode()->mknod(
      "newfile.txt"_pc, S_IFREG | 0660, 0, InvalidationRequired::No);
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

  DesiredMetadata desired;
  int modebits = 07673;
  desired.mode = modebits; // setattr ignores format flags

  auto treeResult =
      treeInode->setattr(desired, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_EQ(treeInode->getNodeId().get(), treeResult.st_ino);
  EXPECT_EQ(S_IFDIR | modebits, treeResult.st_mode);

  auto fileResult =
      fileInode->setattr(desired, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_EQ(fileInode->getNodeId().get(), fileResult.st_ino);
  EXPECT_EQ(S_IFREG | modebits, fileResult.st_mode);
}

TEST(EdenMount, testCanChownFilesAndDirs) {
  TestMount testMount;
  auto builder = FakeTreeBuilder();
  builder.setFile("dir/file.txt", "contents");
  testMount.initialize(builder);

  auto treeInode = testMount.getTreeInode("dir");
  auto fileInode = testMount.getFileInode("dir/file.txt");

  DesiredMetadata desired;
  desired.uid = 23;
  desired.gid = 27;

  auto treeResult =
      treeInode->setattr(desired, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_EQ(treeInode->getNodeId().get(), treeResult.st_ino);
  EXPECT_EQ(desired.uid.value(), treeResult.st_uid);
  EXPECT_EQ(desired.gid.value(), treeResult.st_gid);

  auto fileResult =
      fileInode->setattr(desired, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_EQ(fileInode->getNodeId().get(), fileResult.st_ino);
  EXPECT_EQ(desired.uid.value(), fileResult.st_uid);
  EXPECT_EQ(desired.gid.value(), fileResult.st_gid);
}

TEST(EdenMount, ensureDirectoryExists) {
  auto builder = FakeTreeBuilder{};
  builder.mkdir("sub/foo/bar");
  builder.setFile("sub/file.txt", "");
  TestMount testMount{builder};
  auto edenMount = testMount.getEdenMount();

  edenMount
      ->ensureDirectoryExists(
          "sub/foo/bar"_relpath, ObjectFetchContext::getNullContext())
      .get(0ms);
  EXPECT_NE(nullptr, testMount.getTreeInode("sub/foo/bar"));

  edenMount
      ->ensureDirectoryExists(
          "sub/other/stuff/here"_relpath, ObjectFetchContext::getNullContext())
      .get(0ms);
  EXPECT_NE(nullptr, testMount.getTreeInode("sub/other/stuff/here"));

  auto f1 = edenMount->ensureDirectoryExists(
      "sub/file.txt/baz"_relpath, ObjectFetchContext::getNullContext());
  EXPECT_THROW(std::move(f1).get(0ms), std::system_error);

  auto f2 = edenMount->ensureDirectoryExists(
      "sub/file.txt"_relpath, ObjectFetchContext::getNullContext());
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
        edenMount
            ->ensureDirectoryExists(
                dirPath, ObjectFetchContext::getNullContext())
            .get(0ms);
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
  auto st = fileInode->stat(ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_EQ(st.st_uid, uid);
  EXPECT_EQ(st.st_gid, gid);
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
    auto file = testMount_->getInode("file.txt"_relpath);
    // Load the file into the inode map
    file->incFsRefcount();
    file->getNodeId();
    return file->getNodeId();
  }

  void expectChownSucceeded() {
    auto st = testMount_->getFileInode("file.txt")
                  ->stat(ObjectFetchContext::getNullContext())
                  .get(0ms);
    EXPECT_EQ(st.st_uid, uid);
    EXPECT_EQ(st.st_gid, gid);
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
  inodeMap->decFsRefcount(fileIno, 1);
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
  auto startChannelFuture = mount->startChannel(false);

  mount.reset();
  fuse->close();

  // TODO(strager): Ensure mount is only destroyed after startChannelFuture is
  // ready. (I.e. if FuseChannel::initialize is in progress,
  // EdenMount::~EdenMount should not be called.)

  logAndSwallowExceptions([&] { std::move(startChannelFuture).get(kTimeout); });
  EXPECT_TRUE(mountDestroyDetector.mountIsAlive())
      << "Eden mount should be alive during EdenMount::destroy despite failure in startChannel";

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
      { mount.startChannel(false).get(kTimeout); },
      MockMountDelegate::MountFailed);
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

  auto startChannelFuture = mount.startChannel(false);
  fuse->sendInitRequest();
  fuse->recvResponse();
  std::move(startChannelFuture)
      .within(kTimeout)
      .getVia(testMount.getServerExecutor().get());
  SCOPE_EXIT {
    fuse->close();
    mount.getChannelCompletionFuture().within(kTimeout).getVia(
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

  auto startChannelFuture = mount.startChannel(false);
  fuse->sendInitRequest();
  fuse->recvResponse();
  std::move(startChannelFuture)
      .within(kTimeout)
      .getVia(testMount.getServerExecutor().get());
  SCOPE_EXIT {
    fuse->close();
    mount.getChannelCompletionFuture().within(kTimeout).getVia(
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

  auto startChannelFuture = mount.startChannel(false);
  fuse->sendInitRequest();
  fuse->recvResponse();
  std::move(startChannelFuture)
      .within(kTimeout)
      .getVia(testMount.getServerExecutor().get());

  mount.unmount().get(kTimeout);
  SCOPE_EXIT {
    mount.getChannelCompletionFuture().within(kTimeout).getVia(
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
    mount.getChannelCompletionFuture().within(kTimeout).getVia(
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

  auto startChannelFuture = mount.startChannel(false);
  auto unmountFuture = mount.unmount();

  auto unmountCallCountBeforeMountFails =
      mountDelegate->getFuseUnmountCallCount();
  mountPromise.setException(MockMountDelegate::MountFailed{});

  EXPECT_THROW(
      std::move(startChannelFuture).get(kTimeout),
      MockMountDelegate::MountFailed);
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

  auto startChannelFuture = mount.startChannel(false);
  auto unmountFuture = mount.unmount();
  SCOPE_EXIT {
    std::move(unmountFuture).get(kTimeout);
  };

  auto unmountCallCountBeforeMountCompletes =
      mountDelegate->getFuseUnmountCallCount();
  auto fuse = std::make_shared<FakeFuse>();
  mountPromise.setValue(fuse->start());

  EXPECT_THROW(
      std::move(startChannelFuture).within(kTimeout).get(),
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

  auto startChannelFuture = mount.startChannel(false);
  auto unmountFuture = mount.unmount();
  SCOPE_EXIT {
    std::move(unmountFuture).get(kTimeout);
  };

  auto fuse = std::make_shared<FakeFuse>();
  mountPromise.setValue(fuse->start());

  EXPECT_FALSE(startChannelFuture.wait(kMicroTimeout).isReady())
      << "star should wait until fuseUnmount completes";
  unmountPromise.setValue();
  EXPECT_TRUE(startChannelFuture.wait(kTimeout).isReady())
      << "start should complete after fuseUnmount completes";
}

TEST(EdenMount, unmountWaitsForInProgressMountBeforeUnmounting) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto mountDelegate = std::make_shared<MockMountDelegate>();
  auto mountPromise = mountDelegate->makeMountPromise();
  mountDelegate->makeUnmountSucceed();
  testMount.getPrivHelper()->registerMountDelegate(
      mount.getPath(), mountDelegate);

  auto startChannelFuture = mount.startChannel(false);
  auto unmountFuture = mount.unmount();

  EXPECT_FALSE(mountDelegate->wasFuseUnmountEverCalled())
      << "unmount should not call fuseUnmount until fuseMount completes";
  ASSERT_FALSE(unmountFuture.wait(kMicroTimeout).isReady())
      << "unmount should not finish until fuseMount completes";

  auto fuse = std::make_shared<FakeFuse>();
  mountPromise.setValue(fuse->start());

  try {
    std::move(startChannelFuture).within(kTimeout).get();
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

  auto startChannelFuture = mount.startChannel(false);
  ASSERT_FALSE(startChannelFuture.wait(kMicroTimeout).isReady())
      << "start should not finish before FUSE handshake";

  auto unmountFuture = mount.unmount();
  EXPECT_THROW(
      std::move(startChannelFuture).get(kTimeout),
      FuseDeviceUnmountedDuringInitialization)
      << "unmount should cancel start";

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

  EXPECT_THROW(mount.startChannel(false).get(kTimeout), EdenMountCancelled);
  EXPECT_FALSE(mountDelegate->wasFuseMountEverCalled())
      << "start should fail and not call fuseMount";
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

  auto startChannelFuture = mount.startChannel(false);
  SCOPE_EXIT {
    mountPromise.setException(MockMountDelegate::MountFailed{});
    logAndSwallowExceptions(
        [&] { std::move(startChannelFuture).get(kTimeout); });
  };
  EXPECT_FALSE(startChannelFuture.wait(kMicroTimeout).isReady())
      << "start should not finish before FUSE mounting completes";
  EXPECT_EQ(mount.getState(), EdenMount::State::STARTING);
}

TEST(EdenMountState, mountIsStartingBeforeFuseInitializationCompletes) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto fuse = std::make_shared<FakeFuse>();
  testMount.registerFakeFuse(fuse);

  auto startChannelFuture = mount.startChannel(false);
  SCOPE_EXIT {
    fuse->close();
    logAndSwallowExceptions(
        [&] { std::move(startChannelFuture).get(kTimeout); });
  };
  EXPECT_FALSE(startChannelFuture.wait(kMicroTimeout).isReady())
      << "start should not finish before FUSE initialization completes";
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
      oldMount.getChannelCompletionFuture().within(kTimeout).getVia(
          oldTestMount.getServerExecutor().get());
  oldMount.shutdown(/*doTakeover=*/true).get(kTimeout);
  auto& fuseChannelData = std::get<FuseChannelData>(takeoverData.channelInfo);
  newMount.takeoverFuse(std::move(fuseChannelData));

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

  logAndSwallowExceptions([&] { mount.startChannel(false).get(kTimeout); });
  EXPECT_EQ(mount.getState(), EdenMount::State::FUSE_ERROR);
}

TEST(EdenMountState, mountIsFuseErrorAfterFuseInitializationFails) {
  auto testMount = TestMount{FakeTreeBuilder{}};
  auto& mount = *testMount.getEdenMount();
  auto fuse = std::make_shared<FakeFuse>();
  testMount.registerFakeFuse(fuse);

  auto startChannelFuture = mount.startChannel(false);
  EXPECT_FALSE(startChannelFuture.wait(kMicroTimeout).isReady())
      << "start should not finish before FUSE mounting completes";

  fuse->close();
  logAndSwallowExceptions([&] { std::move(startChannelFuture).get(kTimeout); });

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

  auto shutdownFuture = shutdownFutures.getFuture();

  executor->drain();
  EXPECT_FALSE(shutdownFuture.isReady());
  EXPECT_EQ(EdenMount::State::SHUTTING_DOWN, mount.getState());

  inode.reset();
  executor->drain();
  EXPECT_TRUE(shutdownFuture.isReady());
  EXPECT_EQ(EdenMount::State::SHUT_DOWN, mount.getState());
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
  XCHECK(!mountFuture_.valid())
      << __func__ << " unexpectedly called more than once";
  XCHECK(!wasFuseMountEverCalled())
      << __func__ << " unexpectedly called after fuseMount was called";
  mountFuture_ = folly::makeFuture(std::move(fuseDevice));
}

void MockMountDelegate::makeMountFail() {
  XCHECK(!mountFuture_.valid())
      << __func__ << " unexpectedly called more than once";
  XCHECK(!wasFuseMountEverCalled())
      << __func__ << " unexpectedly called after fuseMount was called";
  mountFuture_ = folly::makeFuture<folly::File>(MountFailed{});
}

folly::Promise<folly::File> MockMountDelegate::makeMountPromise() {
  XCHECK(!mountFuture_.valid())
      << __func__ << " unexpectedly called more than once";
  auto promise = folly::Promise<folly::File>{};
  mountFuture_ = promise.getFuture();
  return promise;
}

void MockMountDelegate::makeUnmountSucceed() {
  XCHECK(!unmountFuture_) << __func__ << " unexpectedly called more than once";
  XCHECK(!wasFuseUnmountEverCalled())
      << __func__ << " unexpectedly called after fuseUnmount was called";
  unmountFuture_.emplace(folly::makeFuture());
}

void MockMountDelegate::makeUnmountFail() {
  XCHECK(!unmountFuture_) << __func__ << " unexpectedly called more than once";
  XCHECK(!wasFuseUnmountEverCalled())
      << __func__ << " unexpectedly called after fuseUnmount was called";
  unmountFuture_.emplace(folly::makeFuture<folly::Unit>(UnmountFailed{}));
}

folly::Promise<folly::Unit> MockMountDelegate::makeUnmountPromise() {
  XCHECK(!unmountFuture_) << __func__ << " unexpectedly called more than once";
  XCHECK(!wasFuseUnmountEverCalled())
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
  XCHECK(inode);
  return EdenMountShutdownBlocker{std::move(inode)};
}

void EdenMountShutdownBlocker::allowShutdownToComplete() {
  inode.reset();
}

EdenMountShutdownBlocker::EdenMountShutdownBlocker(InodePtr inode) noexcept
    : inode{std::move(inode)} {}
} // namespace

#endif
