/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include <sys/socket.h>
#include <unistd.h>

#include <chrono>
#include <memory>

#include <folly/CancellationToken.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/io/async/EventBase.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/EdenDispatcherFactory.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/nfs/Nfsd3.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;

namespace {

constexpr auto kTimeout = std::chrono::seconds{10};

/**
 * Exercises the NFS invalidation pass of inode GC
 * (TreeInode::invalidateChildrenNotMaterializedNFS) against a real Nfsd3
 * attached to a TestMount.
 *
 * The Nfsd3 is only used for its invalidation machinery. It invalidates a
 * directory by chmod'ing the directory's path under the mount path, which
 * here is a plain directory on local disk, so a directory's invalidation
 * succeeds only when the test created that directory on disk first.
 */
class NfsGcTest : public ::testing::Test {
 protected:
  void SetUp() override {
    builder_.setFile("parent/child/one.txt", "1\n");
    builder_.setFile("parent/child/two.txt", "2\n");
    builder_.setFile("parent/sibling.txt", "3\n");
    testMount_ = std::make_unique<TestMount>(builder_);
    attachNfsChannel();

    // Load every inode and give each an FS reference, as if the NFS client
    // had looked them all up.
    testMount_->getEdenMount()->getRootInode()->incFsRefcount();
    for (const char* dir : {"parent", "parent/child"}) {
      testMount_->getTreeInode(dir)->incFsRefcount();
    }
    for (const char* file :
         {"parent/child/one.txt",
          "parent/child/two.txt",
          "parent/sibling.txt"}) {
      testMount_->getFileInode(file)->incFsRefcount();
    }
  }

  void TearDown() override {
    // SetUp may have failed before attachNfsChannel() ran.
    if (clientFd_ != -1) {
      close(clientFd_);
    }
    if (stopFuture_.valid()) {
      // The Nfsd3 stops once it observes the socket's EOF, asynchronously.
      auto deadline = std::chrono::steady_clock::now() + kTimeout;
      while (!stopFuture_.isReady() &&
             std::chrono::steady_clock::now() < deadline) {
        manualExecutor_->run();
        evb_.loopOnce(EVLOOP_NONBLOCK);
      }
      EXPECT_TRUE(stopFuture_.isReady());
    }
    testMount_.reset();
    // Nfsd3 deletes itself on its EventBase.
    evb_.loopOnce(EVLOOP_NONBLOCK);
  }

  void attachNfsChannel() {
    auto& mount = *testMount_->getEdenMount();
    auto& serverState = *testMount_->getServerState();
    manualExecutor_ = std::make_shared<folly::ManualExecutor>();
    auto nfsd3 = std::unique_ptr<Nfsd3, FsChannelDeleter>(new Nfsd3(
        /*privHelper=*/nullptr,
        mount.getPath(),
        &evb_,
        manualExecutor_,
        EdenDispatcherFactory::makeNfsDispatcher(&mount),
        &mount.getStraceLogger(),
        serverState.getProcessInfoCache(),
        serverState.getFsEventLogger(),
        serverState.getEdenFsEventsLogger(),
        serverState.getErrorLogger(),
        /*requestTimeout=*/std::chrono::seconds{30},
        serverState.getNotifier(),
        CaseSensitivity::Sensitive,
        /*readIoSize=*/16 * 1024,
        /*writeIoSize=*/16 * 1024,
        /*maximumInFlightRequests=*/1000,
        /*highNfsRequestsLogInterval=*/std::chrono::minutes{10},
        /*longRunningFSRequestThreshold=*/std::chrono::nanoseconds{0},
        /*traceBusCapacity=*/1000,
        /*fastPathRPCs=*/false,
        serverState.getReloadableConfig()));

    int fds[2];
    PCHECK(0 == socketpair(AF_UNIX, SOCK_STREAM, 0, fds));
    nfsd3->initialize(folly::File{fds[0], /*ownsFd=*/true});
    clientFd_ = fds[1];
    stopFuture_ = nfsd3->getStopFuture();
    mount.setTestFsChannel(std::move(nfsd3));
  }

  /**
   * Run the invalidation pass of GC and return what it reports as the number
   * of invalidated entries.
   */
  uint64_t runGc(std::chrono::system_clock::time_point cutoff) {
    auto* executor = testMount_->getServerExecutor().get();
    return testMount_->getEdenMount()
        ->getRootInode()
        ->handleChildrenNotAccessedRecently(
            cutoff,
            ObjectFetchContext::getNullContext(),
            /*pressureBased=*/true)
        .semi()
        .via(executor)
        .within(kTimeout)
        .getVia(executor);
  }

  /**
   * Run the unload sweep that follows the invalidation pass in GC.
   */
  size_t sweep() {
    return testMount_->getEdenMount()
        ->getRootInode()
        ->unloadChildrenUnreferencedByFs();
  }

  InodeNumber inodeNumberOf(folly::StringPiece path) {
    return testMount_->getInode(path)->getNodeId();
  }

  bool isLoaded(InodeNumber ino) {
    auto* inodeMap = testMount_->getEdenMount()->getInodeMap();
    return inodeMap->isInodeLoadedOrRemembered(ino) &&
        !inodeMap->isInodeRemembered(ino);
  }

  FakeTreeBuilder builder_;
  folly::EventBase evb_;
  std::shared_ptr<folly::ManualExecutor> manualExecutor_;
  std::unique_ptr<TestMount> testMount_;
  folly::SemiFuture<FsStopDataPtr> stopFuture_{
      folly::SemiFuture<FsStopDataPtr>::makeEmpty()};
  int clientFd_{-1};
};

} // namespace

TEST_F(NfsGcTest, failedInvalidationIsNotCountedAsProgress) {
  auto one = inodeNumberOf("parent/child/one.txt");
  auto two = inodeNumberOf("parent/child/two.txt");
  auto sibling = inodeNumberOf("parent/sibling.txt");

  // Neither "parent" nor "parent/child" exists on disk, so the chmod that
  // invalidates each of them fails with ENOENT and the callback that clears
  // their children's FS references never runs.
  auto numInvalidated = runGc(std::chrono::system_clock::time_point::max());
  sweep();

  // FIXME: GC reports both directories as invalidated even though neither
  // chmod succeeded. Nothing had its FS reference cleared, so it should
  // report no progress.
  EXPECT_EQ(2, numInvalidated);
  EXPECT_TRUE(isLoaded(one));
  EXPECT_TRUE(isLoaded(two));
  EXPECT_TRUE(isLoaded(sibling));
}

#endif
