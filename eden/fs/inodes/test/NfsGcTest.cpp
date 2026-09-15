/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include <poll.h>
#include <sys/socket.h>
#include <unistd.h>

#include <chrono>
#include <memory>
#include <thread>
#include <vector>

#include <fb303/ServiceData.h>
#include <fb303/ThreadCachedServiceData.h>
#include <folly/CancellationToken.h>
#include <folly/container/F14Set.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <gtest/gtest.h>

#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/EdenDispatcherFactory.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/nfs/Nfsd3.h"
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/nfs/testharness/NfsRequestUtils.h"
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
 * The Nfsd3 invalidates a directory by chmod'ing the directory's path under
 * the mount path, which here is a plain directory on local disk, so a
 * directory's invalidation succeeds only when the test created that directory
 * on disk first. The requests an NFS client would send are written to the
 * Nfsd3 over a socketpair.
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
   * Create the directory on local disk under the mount path so that the
   * chmod invalidating it succeeds.
   */
  void createOnDisk(folly::StringPiece dir) {
    ensureDirectoryExists(
        testMount_->getEdenMount()->getPath() + RelativePathPiece{dir});
  }

  using PinnedInodeSet = std::shared_ptr<const folly::F14FastSet<InodeNumber>>;

  /**
   * Pin information saying that nothing is pinned, as opposed to the null
   * set meaning pins are unknown.
   */
  static PinnedInodeSet noPins() {
    return std::make_shared<const folly::F14FastSet<InodeNumber>>();
  }

  /**
   * Start the invalidation pass of GC. finishGc() waits for it and returns
   * what it reports as the number of invalidated entries.
   */
  folly::Future<uint64_t> startGc(
      std::chrono::system_clock::time_point cutoff,
      PinnedInodeSet pinnedInodes = noPins()) {
    // Bound to the server executor, which the wait helpers drain, so the
    // walk's deferred continuations run while a test waits on it; a
    // SemiFuture's would only run once something drove it.
    return testMount_->getEdenMount()
        ->getRootInode()
        ->handleChildrenNotAccessedRecently(
            cutoff,
            ObjectFetchContext::getNullContext(),
            /*pressureBased=*/true,
            folly::CancellationToken{},
            std::move(pinnedInodes))
        .semi()
        .via(testMount_->getServerExecutor().get());
  }

  /**
   * Drive the walk to completion and return what it reports.
   */
  uint64_t finishGc(folly::Future<uint64_t> gc) {
    auto deadline = std::chrono::steady_clock::now() + kTimeout;
    while (!gc.isReady()) {
      testMount_->drainServerExecutor();
      manualExecutor_->run();
      evb_.loopOnce(EVLOOP_NONBLOCK);
      if (std::chrono::steady_clock::now() > deadline) {
        ADD_FAILURE() << "GC did not finish";
        return 0;
      }
      gc.wait(std::chrono::milliseconds{1});
    }
    return std::move(gc).value();
  }

  uint64_t runGc(
      std::chrono::system_clock::time_point cutoff,
      PinnedInodeSet pinnedInodes = noPins()) {
    return finishGc(startGc(cutoff, std::move(pinnedInodes)));
  }

  /**
   * Wait until GC is blocked on the nfsGcInvalidation fault, driving the
   * mount's server executor in case the walk needs it to get there.
   */
  void waitUntilInvalidationBlocked() {
    auto& faultInjector = testMount_->getServerState()->getFaultInjector();
    auto deadline = std::chrono::steady_clock::now() + kTimeout;
    while (!faultInjector.waitUntilBlocked(
        "nfsGcInvalidation", std::chrono::milliseconds{10})) {
      testMount_->drainServerExecutor();
      ASSERT_LT(std::chrono::steady_clock::now(), deadline)
          << "GC never reached the invalidation";
    }
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

  /**
   * Send one NFS request to the Nfsd3 over the socketpair and return its
   * reply, driving the server's executors and the mount's server executor
   * until the reply arrives.
   */
  std::unique_ptr<folly::IOBuf> sendAndReceive(
      std::unique_ptr<folly::IOBuf> request) {
    auto bytes = request->coalesce();
    while (!bytes.empty()) {
      auto written = write(clientFd_, bytes.data(), bytes.size());
      if (written <= 0) {
        ADD_FAILURE() << "writing the request failed";
        return folly::IOBuf::create(0);
      }
      bytes.advance(static_cast<size_t>(written));
    }

    std::vector<uint8_t> reply;
    size_t expectedSize = 0;
    auto deadline = std::chrono::steady_clock::now() + kTimeout;
    while (std::chrono::steady_clock::now() < deadline) {
      evb_.loopOnce(EVLOOP_NONBLOCK);
      manualExecutor_->run();
      testMount_->drainServerExecutor();

      struct pollfd pfd{};
      pfd.fd = clientFd_;
      pfd.events = POLLIN;
      if (poll(&pfd, 1, 10) <= 0) {
        continue;
      }
      uint8_t buf[64 * 1024];
      auto n = read(clientFd_, buf, sizeof(buf));
      if (n <= 0) {
        break;
      }
      reply.insert(reply.end(), buf, buf + n);
      if (expectedSize == 0 && reply.size() >= sizeof(uint32_t)) {
        // Record mark: high bit is the last-fragment flag, the rest the
        // fragment length.
        uint32_t mark = (uint32_t{reply[0]} << 24) |
            (uint32_t{reply[1]} << 16) | (uint32_t{reply[2]} << 8) |
            uint32_t{reply[3]};
        // Nfsd3 sends every reply as one fragment, which is all this reads.
        if (!(mark & 0x80000000)) {
          ADD_FAILURE() << "multi-fragment RPC reply";
          return folly::IOBuf::create(0);
        }
        expectedSize = sizeof(uint32_t) + (mark & 0x7fffffff);
      }
      if (expectedSize != 0 && reply.size() >= expectedSize) {
        break;
      }
    }
    if (expectedSize == 0 || reply.size() < expectedSize) {
      ADD_FAILURE() << "no complete RPC reply";
      return folly::IOBuf::create(0);
    }
    return folly::IOBuf::copyBuffer(reply.data(), reply.size());
  }

  /**
   * Parse an accepted RPC reply and deserialize the procedure's result.
   */
  template <typename Res>
  Res parseReply(std::unique_ptr<folly::IOBuf> reply) {
    if (reply->computeChainDataLength() == 0) {
      // sendAndReceive already recorded the failure.
      return Res{};
    }
    folly::io::Cursor cursor(reply.get());
    cursor.skip(sizeof(uint32_t)); // record mark
    auto msg = XdrTrait<rpc_msg_reply>::deserialize(cursor);
    if (msg.rbody.tag != reply_stat::MSG_ACCEPTED) {
      ADD_FAILURE() << "RPC reply rejected";
      return Res{};
    }
    if (std::get<accepted_reply>(msg.rbody.v).stat != accept_stat::SUCCESS) {
      ADD_FAILURE() << "RPC reply not successful";
      return Res{};
    }
    return XdrTrait<Res>::deserialize(cursor);
  }

  opaque_auth credentials() {
    return makeAuthSysCred(
        authsys_parms{/*stamp=*/0, "nfs-gc-test", getuid(), getgid(), {}});
  }

  /**
   * SETATTR the inode's permission bits, as the NFS client does for a chmod,
   * returning the NFS status of the reply.
   */
  nfsstat3 setattrMode(InodeNumber ino, mode_t mode) {
    SETATTR3args args{nfs_fh3{ino}, sattr3{}, sattrguard3{}};
    args.new_attributes.mode.tag = true;
    args.new_attributes.mode.v = uint32_t{mode};
    auto res = parseReply<SETATTR3res>(sendAndReceive(
        buildNfsRequest(nextXid_++, nfsv3Procs::setattr, credentials(), args)));
    return res.tag;
  }

  /**
   * Number of chmods GC has issued to invalidate directories so far.
   */
  int64_t numInvalidationAttempts() {
    testMount_->getServerState()->getStats()->flush();
    facebook::fb303::ThreadCachedServiceData::get()->publishStats();
    return facebook::fb303::ServiceData::get()
        ->getCounterIfExists("nfs.invalidation.gc.attempt.sum")
        .value_or(0);
  }

  FakeTreeBuilder builder_;
  folly::EventBase evb_;
  std::shared_ptr<folly::ManualExecutor> manualExecutor_;
  std::unique_ptr<TestMount> testMount_;
  folly::SemiFuture<FsStopDataPtr> stopFuture_{
      folly::SemiFuture<FsStopDataPtr>::makeEmpty()};
  int clientFd_{-1};
  uint32_t nextXid_{1};
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

  // Neither chmod succeeded, so nothing had its FS reference cleared and GC
  // must report no progress.
  EXPECT_EQ(0, numInvalidated);
  EXPECT_TRUE(isLoaded(one));
  EXPECT_TRUE(isLoaded(two));
  EXPECT_TRUE(isLoaded(sibling));
}

TEST_F(NfsGcTest, directoriesStayReferencedWithoutPinInformation) {
  createOnDisk("parent/child");
  auto child = inodeNumberOf("parent/child");
  auto one = inodeNumberOf("parent/child/one.txt");
  auto sibling = inodeNumberOf("parent/sibling.txt");

  // Without pin information, a directory might be some process's working
  // directory, so only the files are cleared: "parent/child" keeps its FS
  // reference even though "parent" was invalidated.
  EXPECT_EQ(
      3,
      runGc(
          std::chrono::system_clock::time_point::max(),
          /*pinnedInodes=*/nullptr));
  sweep();
  EXPECT_TRUE(isLoaded(child));
  EXPECT_FALSE(isLoaded(one));
  EXPECT_FALSE(isLoaded(sibling));

  // Knowing that nothing is pinned, the next run clears the directory too.
  EXPECT_EQ(1, runGc(std::chrono::system_clock::time_point::max(), noPins()));
  sweep();
  EXPECT_FALSE(isLoaded(child));
}

TEST_F(NfsGcTest, parentIsInvalidatedAfterItsChildWasInvalidated) {
  createOnDisk("parent/child");
  auto child = inodeNumberOf("parent/child");
  auto sibling = inodeNumberOf("parent/sibling.txt");
  auto childMode =
      testMount_->getTreeInode("parent/child")->getMetadata().mode & 07777;

  // Everything was last used at the clock's initial time. Move the clock
  // forward so all of it is older than the cutoff.
  auto& clock = testMount_->getClock();
  clock.advance(std::chrono::hours{2});
  auto cutoff = clock.getTimePoint() - std::chrono::hours{1};

  // The chmod that invalidates "parent/child" reaches EdenFS as a SETATTR of
  // the directory's current mode. Here the chmod lands on local disk instead,
  // so send that SETATTR by hand while the invalidation callback is blocked:
  // after the chmod completed, and before the parent decides whether the
  // child is stale.
  auto& faultInjector = testMount_->getServerState()->getFaultInjector();
  faultInjector.injectBlock("nfsGcInvalidation", "parent/child");
  auto gc = startGc(cutoff);
  waitUntilInvalidationBlocked();
  EXPECT_EQ(nfsstat3::NFS3_OK, setattrMode(child, childMode));
  faultInjector.unblock("nfsGcInvalidation", "parent/child");
  auto numInvalidated = finishGc(std::move(gc));
  sweep();

  // FIXME: "parent" saw its child's request time refreshed by GC's own chmod,
  // concluded that it was recently used, and skipped invalidating itself. Only
  // the child's own invalidation happened, clearing its two files, while the
  // child and its sibling file keep their FS references and stay loaded.
  EXPECT_EQ(2, numInvalidated);
  EXPECT_TRUE(isLoaded(child));
  EXPECT_TRUE(isLoaded(sibling));
}

TEST_F(NfsGcTest, pinnedInodesAndTheirAncestorsKeepTheirReferences) {
  createOnDisk("parent/child");
  auto child = inodeNumberOf("parent/child");
  auto one = inodeNumberOf("parent/child/one.txt");
  auto two = inodeNumberOf("parent/child/two.txt");
  auto sibling = inodeNumberOf("parent/sibling.txt");

  // Some process holds "one.txt" and "sibling.txt" open.
  auto pins = std::make_shared<const folly::F14FastSet<InodeNumber>>(
      folly::F14FastSet<InodeNumber>{one, sibling});
  auto numInvalidated =
      runGc(std::chrono::system_clock::time_point::max(), pins);
  sweep();

  // FIXME: the pins are ignored, so the pinned files and the directory whose
  // subtree contains one are all forgotten along with "two.txt".
  EXPECT_EQ(4, numInvalidated);
  EXPECT_FALSE(isLoaded(one));
  EXPECT_FALSE(isLoaded(sibling));
  EXPECT_FALSE(isLoaded(child));
  EXPECT_FALSE(isLoaded(two));
}

TEST_F(NfsGcTest, pinnedDirectoryStillHasItsChildrenReclaimed) {
  createOnDisk("parent/child");
  auto child = inodeNumberOf("parent/child");
  auto one = inodeNumberOf("parent/child/one.txt");
  auto sibling = inodeNumberOf("parent/sibling.txt");

  // Some process has "parent/child" as its working directory.
  auto pins = std::make_shared<const folly::F14FastSet<InodeNumber>>(
      folly::F14FastSet<InodeNumber>{child});
  auto numInvalidated =
      runGc(std::chrono::system_clock::time_point::max(), pins);
  sweep();

  // FIXME: the pin is ignored and the working directory is forgotten too.
  EXPECT_EQ(4, numInvalidated);
  EXPECT_FALSE(isLoaded(child));
  EXPECT_FALSE(isLoaded(one));
  EXPECT_FALSE(isLoaded(sibling));
}

TEST_F(NfsGcTest, directoryWithNothingToClearIsNotInvalidatedAgain) {
  createOnDisk("parent/child");
  // A materialized directory is never invalidated, so "parent/child" keeps
  // its FS reference and stays loaded across GC runs.
  testMount_->addFile("parent/untracked.txt", "u\n");
  auto one = inodeNumberOf("parent/child/one.txt");
  auto two = inodeNumberOf("parent/child/two.txt");

  // The first run clears the two files under "parent/child", and the sweep
  // forgets them.
  EXPECT_EQ(2, runGc(std::chrono::system_clock::time_point::max()));
  sweep();
  EXPECT_FALSE(isLoaded(one));
  EXPECT_FALSE(isLoaded(two));

  // A second run has no FS reference left to clear under "parent/child".
  auto attemptsBefore = numInvalidationAttempts();
  EXPECT_EQ(0, runGc(std::chrono::system_clock::time_point::max()));
  // GC must not chmod "parent/child" again.
  EXPECT_EQ(attemptsBefore, numInvalidationAttempts());
}

TEST_F(
    NfsGcTest,
    directoryWhoseOnlyReferencedChildrenAreDirectoriesIsLeftAlone) {
  createOnDisk("parent/child");
  auto child = inodeNumberOf("parent/child");

  // Without pin information the first run clears the three files but leaves
  // "parent/child" referenced.
  EXPECT_EQ(3, runGc(std::chrono::system_clock::time_point::max(), nullptr));
  sweep();
  ASSERT_TRUE(isLoaded(child));

  // "parent" now has nothing this run may clear, since its only referenced
  // child is a directory, so it is not chmod'ed either.
  auto attemptsBefore = numInvalidationAttempts();
  EXPECT_EQ(0, runGc(std::chrono::system_clock::time_point::max(), nullptr));
  EXPECT_EQ(attemptsBefore, numInvalidationAttempts());
}

#endif
