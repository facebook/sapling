/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/Nfsd3.h"

#include <poll.h>
#include <sys/socket.h>
#include <unistd.h>

#include <cstring>

#include <fb303/ServiceData.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/IOBufQueue.h>
#include <folly/logging/Logger.h>
#include <gtest/gtest.h>

#include "eden/common/telemetry/SessionInfo.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/nfs/NfsDispatcher.h"
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/ErrorLogger.h"
#include "eden/fs/telemetry/test/CapturingScribeLogger.h"
#include "eden/fs/utils/Clock.h"

namespace {

using namespace facebook::eden;

/**
 * A minimal NfsDispatcher for driving a real Nfsd3 over a socketpair.
 *
 * getattr succeeds with a fixed directory stat; everything else fails
 * with ENOENT.
 */
class FakeNfsDispatcher : public NfsDispatcher {
 public:
  FakeNfsDispatcher(EdenStatsPtr stats, const Clock& clock)
      : NfsDispatcher(std::move(stats), clock) {}

  ImmediateFuture<struct stat> getattr(
      InodeNumber /*ino*/,
      const ObjectFetchContextPtr& /*context*/) override {
    struct stat st{};
    st.st_mode = S_IFDIR | 0755;
    st.st_nlink = 2;
    st.st_size = 4096;
    return st;
  }

  ImmediateFuture<folly::Unit> updateLastFsRequestTime(
      InodeNumber /*ino*/) override {
    return folly::unit;
  }

  // NfsDispatcher's operations are all pure virtual, but the auth tests
  // only ever reach getattr, so every other operation is a one-line
  // ENOENT stub.
  using Ino = InodeNumber;
  using Name = PathComponent;
  using Ctx = const ObjectFetchContextPtr&;
  using LookupRes = std::tuple<InodeNumber, struct stat>;
#define ENOENT_STUB(name, Res, ...)                          \
  ImmediateFuture<Res> name(__VA_ARGS__) override {          \
    return makeImmediateFuture<Res>(                         \
        std::system_error(ENOENT, std::generic_category())); \
  }
  ENOENT_STUB(setattr, SetattrRes, Ino, DesiredMetadata, Ctx)
  ENOENT_STUB(getParent, InodeNumber, Ino, Ctx)
  ENOENT_STUB(lookup, LookupRes, Ino, Name, Ctx)
  ENOENT_STUB(readlink, std::string, Ino, Ctx)
  ENOENT_STUB(read, ReadRes, Ino, size_t, FileOffset, Ctx)
  ENOENT_STUB(
      write,
      WriteRes,
      Ino,
      std::unique_ptr<folly::IOBuf>,
      FileOffset,
      Ctx)
  ENOENT_STUB(create, CreateRes, Ino, Name, mode_t, createhow3, Ctx)
  ENOENT_STUB(mkdir, MkdirRes, Ino, Name, mode_t, Ctx)
  ENOENT_STUB(symlink, SymlinkRes, Ino, Name, std::string, Ctx)
  ENOENT_STUB(mknod, MknodRes, Ino, Name, mode_t, dev_t, Ctx)
  ENOENT_STUB(unlink, UnlinkRes, Ino, Name, Ctx)
  ENOENT_STUB(rmdir, RmdirRes, Ino, Name, Ctx)
  ENOENT_STUB(rename, RenameRes, Ino, Name, Ino, Name, Ctx)
  ENOENT_STUB(readdir, ReaddirRes, Ino, FileOffset, uint32_t, Ctx)
  ENOENT_STUB(readdirplus, ReaddirRes, Ino, FileOffset, uint32_t, Ctx)
  ENOENT_STUB(statfs, struct statfs, Ino, Ctx)
#undef ENOENT_STUB
};

opaque_auth makeAuthSysCred(const authsys_parms& creds) {
  folly::IOBufQueue queue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender ser(&queue, 256);
  XdrTrait<authsys_parms>::serialize(ser, creds);
  auto buf = queue.move();
  auto bytes = buf->coalesce();
  return opaque_auth{
      auth_flavor::AUTH_SYS, OpaqueBytes{bytes.begin(), bytes.end()}};
}

/**
 * Serialize an NFSv3 request for the given procedure with the given
 * credential, framed with a record-mark fragment header. serializeArgs is
 * called with the QueueAppender to append the procedure arguments.
 */
template <typename SerializeArgs>
std::unique_ptr<folly::IOBuf> buildNfsRequestImpl(
    uint32_t xid,
    nfsv3Procs proc,
    opaque_auth cred,
    SerializeArgs&& serializeArgs) {
  folly::IOBufQueue queue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender ser(&queue, 1024);

  XdrTrait<uint32_t>::serialize(ser, 0); // fragment header placeholder
  rpc_msg_call call{
      xid,
      msg_type::CALL,
      call_body{
          kRPCVersion,
          kNfsdProgNumber,
          kNfsd3ProgVersion,
          folly::to_underlying(proc),
          std::move(cred),
          opaque_auth{auth_flavor::AUTH_NONE, {}},
      },
  };
  XdrTrait<rpc_msg_call>::serialize(ser, call);
  serializeArgs(ser);

  auto len = static_cast<uint32_t>(queue.chainLength() - sizeof(uint32_t));
  auto buf = queue.move();
  auto* header = reinterpret_cast<uint32_t*>(buf->writableData());
  *header = folly::Endian::big(len | 0x80000000);
  return buf;
}

template <typename Args>
std::unique_ptr<folly::IOBuf> buildNfsRequest(
    uint32_t xid,
    nfsv3Procs proc,
    opaque_auth cred,
    const Args& args) {
  return buildNfsRequestImpl(
      xid, proc, std::move(cred), [&](folly::io::QueueAppender& ser) {
        XdrTrait<Args>::serialize(ser, args);
      });
}

std::unique_ptr<folly::IOBuf>
buildNfsRequest(uint32_t xid, nfsv3Procs proc, opaque_auth cred) {
  return buildNfsRequestImpl(
      xid, proc, std::move(cred), [](folly::io::QueueAppender&) {});
}

/**
 * Drives a real Nfsd3 server over a Unix socketpair. The ManualExecutor
 * keeps all request processing on the test thread so state recorded by
 * FakeNfsDispatcher can be read without synchronization.
 */
struct Nfsd3Test : ::testing::Test {
  void SetUp() override {
    config_ = EdenConfig::createTestEdenConfig();
    reloadableConfig_ = std::make_shared<ReloadableConfig>(config_);
    scribeLogger_ = std::make_shared<CapturingScribeLogger>();
    errorLogger_ = std::make_unique<ErrorLogger>(
        scribeLogger_, SessionInfo{}, reloadableConfig_);

    auto dispatcher =
        std::make_unique<FakeNfsDispatcher>(makeRefPtr<EdenStats>(), clock_);
    dispatcher_ = dispatcher.get();

    manualExecutor_ = std::make_shared<folly::ManualExecutor>();
    nfsd3_ = std::unique_ptr<Nfsd3, FsChannelDeleter>(new Nfsd3(
        /*privHelper=*/nullptr,
        canonicalPath("/mnt/nfs-test"),
        &evb_,
        manualExecutor_,
        std::move(dispatcher),
        &straceLogger_,
        std::make_shared<ProcessInfoCache>(),
        /*fsEventLogger=*/nullptr,
        std::make_shared<EdenFsEventsLogger>(nullptr),
        *errorLogger_,
        /*requestTimeout=*/std::chrono::seconds{30},
        /*notifications=*/nullptr,
        CaseSensitivity::Sensitive,
        /*iosize=*/16 * 1024,
        /*maximumInFlightRequests=*/1000,
        /*highNfsRequestsLogInterval=*/std::chrono::minutes{10},
        /*longRunningFSRequestThreshold=*/std::chrono::nanoseconds{0},
        /*traceBusCapacity=*/1000,
        /*fastPathRPCs=*/false,
        reloadableConfig_));

    int fds[2];
    ASSERT_EQ(0, socketpair(AF_UNIX, SOCK_STREAM, 0, fds));
    nfsd3_->initialize(folly::File{fds[0], /*ownsFd=*/true});
    clientFd_ = fds[1];
  }

  void TearDown() override {
    auto stopFuture = nfsd3_->getStopFuture();
    close(clientFd_);
    // Drain the EOF handling so the RPC handler shuts down cleanly before
    // the Nfsd3 (which owns state the processor references) is destroyed.
    for (int i = 0; i < 20 && !stopFuture.isReady(); ++i) {
      manualExecutor_->run();
      evb_.loopOnce(EVLOOP_NONBLOCK);
    }
    EXPECT_TRUE(stopFuture.isReady());
    nfsd3_.reset();
    evb_.loopOnce(EVLOOP_NONBLOCK);
  }

  std::vector<uint8_t> sendAndReceive(std::unique_ptr<folly::IOBuf> request) {
    auto bytes = request->coalesce();
    EXPECT_EQ(
        static_cast<ssize_t>(bytes.size()),
        write(clientFd_, bytes.data(), bytes.size()));
    for (int i = 0; i < 20; ++i) {
      evb_.loopOnce(EVLOOP_NONBLOCK);
      manualExecutor_->run();
    }

    struct pollfd pfd{};
    pfd.fd = clientFd_;
    pfd.events = POLLIN;
    EXPECT_GT(poll(&pfd, 1, 1000), 0) << "Expected an RPC reply";

    uint8_t buf[1024];
    auto nread = read(clientFd_, buf, sizeof(buf));
    EXPECT_GT(nread, 0);
    return std::vector<uint8_t>(buf, buf + nread);
  }

  std::vector<uint8_t> sendGetattr(uint32_t xid, opaque_auth cred) {
    return sendAndReceive(buildNfsRequest(
        xid,
        nfsv3Procs::getattr,
        std::move(cred),
        GETATTR3args{nfs_fh3{InodeNumber{42}}}));
  }

  std::vector<uint8_t> sendAccess(uint32_t xid, opaque_auth cred) {
    return sendAndReceive(buildNfsRequest(
        xid,
        nfsv3Procs::access,
        std::move(cred),
        ACCESS3args{nfs_fh3{InodeNumber{42}}, /*access=*/0x1}));
  }

  std::vector<uint8_t> sendRead(uint32_t xid, opaque_auth cred) {
    return sendAndReceive(buildNfsRequest(
        xid,
        nfsv3Procs::read,
        std::move(cred),
        READ3args{nfs_fh3{InodeNumber{42}}, /*offset=*/0, /*count=*/16}));
  }

  int64_t getCounter(folly::StringPiece key) {
    dispatcher_->getStats()->flush();
    return facebook::fb303::ServiceData::get()
        ->getCounterIfExists(key)
        .value_or(0);
  }

  void setRootMode(NfsAccessMode mode) {
    config_->nfsRootAccessMode.setValue(
        mode, ConfigSourceType::UserConfig, true);
  }

  void setWheelMode(NfsAccessMode mode) {
    config_->nfsWheelAccessMode.setValue(
        mode, ConfigSourceType::UserConfig, true);
  }

  static uint32_t readBigEndianU32(
      const std::vector<uint8_t>& data,
      size_t offset) {
    EXPECT_GE(data.size(), offset + 4);
    uint32_t val;
    memcpy(&val, data.data() + offset, sizeof(val));
    return folly::Endian::big(val);
  }

  /**
   * Assert the reply is MSG_ACCEPTED with accept_stat SUCCESS.
   * Reply layout: fragment(0) xid(4) mtype(8) reply_stat(12) verf(16, 20)
   * accept_stat(24).
   */
  static void expectAcceptedSuccess(const std::vector<uint8_t>& reply) {
    EXPECT_EQ(readBigEndianU32(reply, 8), 1u); // msg_type::REPLY
    EXPECT_EQ(readBigEndianU32(reply, 12), 0u); // reply_stat::MSG_ACCEPTED
    EXPECT_EQ(readBigEndianU32(reply, 24), 0u); // accept_stat::SUCCESS
  }

  folly::EventBase evb_;
  std::shared_ptr<folly::ManualExecutor> manualExecutor_;
  std::shared_ptr<EdenConfig> config_;
  std::shared_ptr<ReloadableConfig> reloadableConfig_;
  std::shared_ptr<CapturingScribeLogger> scribeLogger_;
  std::unique_ptr<ErrorLogger> errorLogger_;
  folly::Logger straceLogger_{"eden.test.nfsd3"};
  UnixClock clock_;
  FakeNfsDispatcher* dispatcher_ = nullptr;
  std::unique_ptr<Nfsd3, FsChannelDeleter> nfsd3_;
  int clientFd_ = -1;
};

TEST_F(Nfsd3Test, root_cred_bumps_both_privileged_counters) {
  auto rootBefore = getCounter("nfs.privileged_access.uid_root.sum.60");
  auto wheelBefore = getCounter("nfs.privileged_access.gid_wheel.sum.60");

  auto reply = sendGetattr(
      4, makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/0, /*gid=*/0, {0}}));
  expectAcceptedSuccess(reply);

  EXPECT_EQ(
      getCounter("nfs.privileged_access.uid_root.sum.60") - rootBefore, 1);
  EXPECT_EQ(
      getCounter("nfs.privileged_access.gid_wheel.sum.60") - wheelBefore, 1);
}

TEST_F(Nfsd3Test, only_wheel_claims_bump_the_wheel_counter) {
  auto rootBefore = getCounter("nfs.privileged_access.uid_root.sum.60");
  auto wheelBefore = getCounter("nfs.privileged_access.gid_wheel.sum.60");

  // A plain user counts toward neither class; a credential whose only
  // privileged claim is an auxiliary gid 0 counts toward wheel alone.
  expectAcceptedSuccess(sendGetattr(
      5, makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/20, {20}})));
  expectAcceptedSuccess(sendGetattr(
      6,
      makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/20, {20, 0}})));

  EXPECT_EQ(getCounter("nfs.privileged_access.uid_root.sum.60"), rootBefore);
  EXPECT_EQ(
      getCounter("nfs.privileged_access.gid_wheel.sum.60") - wheelBefore, 1);
}

TEST_F(Nfsd3Test, both_modes_off_disable_the_counters) {
  setRootMode(NfsAccessMode::Off);
  setWheelMode(NfsAccessMode::Off);
  auto rootBefore = getCounter("nfs.privileged_access.uid_root.sum.60");
  auto wheelBefore = getCounter("nfs.privileged_access.gid_wheel.sum.60");

  auto reply = sendGetattr(
      7, makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/0, /*gid=*/0, {0}}));
  expectAcceptedSuccess(reply);

  EXPECT_EQ(getCounter("nfs.privileged_access.uid_root.sum.60"), rootBefore);
  EXPECT_EQ(getCounter("nfs.privileged_access.gid_wheel.sum.60"), wheelBefore);
}

struct Nfsd3BlockingTest : Nfsd3Test {
  // uid 0, not wheel: exercises nfs:root-access-mode alone.
  static opaque_auth rootOnlyCred() {
    return makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/0, /*gid=*/20, {20}});
  }

  // wheel (primary gid 0), not uid 0: exercises nfs:wheel-access-mode alone.
  static opaque_auth wheelCred() {
    return makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/0, {0}});
  }

  // wheel via the auxiliary gids list only.
  static opaque_auth auxWheelCred() {
    return makeAuthSysCred(
        {/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/20, {20, 0}});
  }

  // claims both identity classes at once.
  static opaque_auth rootAndWheelCred() {
    return makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/0, /*gid=*/0, {0}});
  }

  static opaque_auth userCred() {
    return makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/20, {20}});
  }

  /**
   * Assert the reply is MSG_DENIED with AUTH_ERROR / AUTH_TOOWEAK.
   * Reply layout: fragment(0) xid(4) mtype(8) reply_stat(12) reject_stat(16)
   * auth_stat(20).
   */
  static void expectAuthTooWeak(const std::vector<uint8_t>& reply) {
    EXPECT_EQ(readBigEndianU32(reply, 8), 1u); // msg_type::REPLY
    EXPECT_EQ(readBigEndianU32(reply, 12), 1u); // reply_stat::MSG_DENIED
    EXPECT_EQ(readBigEndianU32(reply, 16), 1u); // reject_stat::AUTH_ERROR
    EXPECT_EQ(
        readBigEndianU32(reply, 20),
        static_cast<uint32_t>(auth_stat::AUTH_TOOWEAK));
  }
};

TEST_F(Nfsd3BlockingTest, nothing_blocked_by_default) {
  expectAcceptedSuccess(sendGetattr(1, rootOnlyCred()));
  expectAcceptedSuccess(sendGetattr(2, wheelCred()));
}

TEST_F(Nfsd3BlockingTest, block_root_rejects_uid0_across_procedures) {
  setRootMode(NfsAccessMode::Block);

  expectAuthTooWeak(sendGetattr(1, rootOnlyCred()));
  expectAuthTooWeak(sendAccess(2, rootOnlyCred()));
  expectAuthTooWeak(sendRead(3, rootOnlyCred()));
  // Wheel is not uid 0; blocking the root class alone leaves it alone.
  expectAcceptedSuccess(sendGetattr(4, wheelCred()));
  expectAcceptedSuccess(sendGetattr(5, userCred()));
}

TEST_F(Nfsd3BlockingTest, block_bumps_privileged_and_blocked_counters) {
  setRootMode(NfsAccessMode::Block);
  auto rootBefore = getCounter("nfs.privileged_access.uid_root.sum.60");
  auto blockedBefore = getCounter("nfs.blocked_access.sum.60");

  expectAuthTooWeak(sendGetattr(1, rootOnlyCred()));
  expectAuthTooWeak(sendGetattr(2, rootOnlyCred()));

  // "block" is a strict superset of "log": rejected requests still bump
  // the class's privileged-access counter.
  EXPECT_EQ(
      getCounter("nfs.privileged_access.uid_root.sum.60") - rootBefore, 2);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum.60") - blockedBefore, 2);
}

TEST_F(Nfsd3BlockingTest, wheel_block_is_independent_of_root_mode) {
  // Root class fully off, wheel class blocking: the independence case.
  setRootMode(NfsAccessMode::Off);
  setWheelMode(NfsAccessMode::Block);
  auto rootBefore = getCounter("nfs.privileged_access.uid_root.sum.60");
  auto wheelBefore = getCounter("nfs.privileged_access.gid_wheel.sum.60");
  auto blockedBefore = getCounter("nfs.blocked_access.sum.60");

  // Both wheel spellings (primary and auxiliary gid 0) are rejected, and a
  // credential claiming root AND wheel is rejected by the wheel class alone.
  expectAuthTooWeak(sendGetattr(1, wheelCred()));
  expectAuthTooWeak(sendGetattr(2, auxWheelCred()));
  expectAuthTooWeak(sendGetattr(3, rootAndWheelCred()));
  // Plain root and plain user are untouched: the root class is off.
  expectAcceptedSuccess(sendGetattr(4, rootOnlyCred()));
  expectAcceptedSuccess(sendGetattr(5, userCred()));

  EXPECT_EQ(getCounter("nfs.privileged_access.uid_root.sum.60"), rootBefore);
  EXPECT_EQ(
      getCounter("nfs.privileged_access.gid_wheel.sum.60") - wheelBefore, 3);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum.60") - blockedBefore, 3);
}

TEST_F(Nfsd3BlockingTest, missing_creds_are_never_blocked) {
  setRootMode(NfsAccessMode::Block);
  setWheelMode(NfsAccessMode::Block);

  expectAcceptedSuccess(
      sendGetattr(1, opaque_auth{auth_flavor::AUTH_NONE, {}}));
}

TEST_F(Nfsd3BlockingTest, null_proc_is_exempt) {
  setRootMode(NfsAccessMode::Block);
  setWheelMode(NfsAccessMode::Block);

  auto reply =
      sendAndReceive(buildNfsRequest(1, nfsv3Procs::null, rootOnlyCred()));
  expectAcceptedSuccess(reply);
}

TEST_F(Nfsd3BlockingTest, control_plane_procs_are_exempt) {
  setRootMode(NfsAccessMode::Block);
  setWheelMode(NfsAccessMode::Block);
  auto rootBefore = getCounter("nfs.privileged_access.uid_root.sum.60");
  auto blockedBefore = getCounter("nfs.blocked_access.sum.60");

  // Mount bookkeeping keeps working for a blocked identity...
  expectAcceptedSuccess(sendAndReceive(buildNfsRequest(
      1,
      nfsv3Procs::fsstat,
      rootAndWheelCred(),
      FSSTAT3args{nfs_fh3{InodeNumber{42}}})));
  expectAcceptedSuccess(sendAndReceive(buildNfsRequest(
      2,
      nfsv3Procs::fsinfo,
      rootAndWheelCred(),
      FSINFO3args{nfs_fh3{InodeNumber{42}}})));
  expectAcceptedSuccess(sendAndReceive(buildNfsRequest(
      3,
      nfsv3Procs::pathconf,
      rootAndWheelCred(),
      PATHCONF3args{nfs_fh3{InodeNumber{42}}})));

  // ...and is neither counted nor blocked...
  EXPECT_EQ(getCounter("nfs.privileged_access.uid_root.sum.60"), rootBefore);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum.60"), blockedBefore);

  // ...while file access is still rejected.
  expectAuthTooWeak(sendGetattr(4, rootAndWheelCred()));
}

TEST_F(Nfsd3BlockingTest, config_changes_apply_without_restart) {
  setRootMode(NfsAccessMode::Block);
  expectAuthTooWeak(sendGetattr(1, rootOnlyCred()));

  // Dropping the mode back to "log" unblocks the same running server.
  setRootMode(NfsAccessMode::Log);
  expectAcceptedSuccess(sendGetattr(2, rootOnlyCred()));

  // The wheel mode is picked up independently.
  setWheelMode(NfsAccessMode::Block);
  expectAuthTooWeak(sendGetattr(3, wheelCred()));
  expectAcceptedSuccess(sendGetattr(4, rootOnlyCred()));
}

} // namespace

#endif
