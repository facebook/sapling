/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/Nfsd3.h"

#include "eden/fs/nfs/NfsAccessRateLimiter.h"

#include <poll.h>
#include <sys/socket.h>
#include <unistd.h>

#include <cstring>
#include <unordered_map>

#include <fb303/ServiceData.h>
#include <fb303/ThreadCachedServiceData.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/IOBufQueue.h>
#include <folly/logging/Logger.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/nfs/NfsDispatcher.h"
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/ErrorLogger.h"
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
    errorLogger_ = std::make_unique<ErrorLogger>();

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
        /*readIoSize=*/16 * 1024,
        /*writeIoSize=*/16 * 1024,
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
    facebook::fb303::ThreadCachedServiceData::get()->publishStats();
    return facebook::fb303::ServiceData::get()
        ->getCounterIfExists(key)
        .value_or(0);
  }

  void setUidModes(std::unordered_map<uint32_t, NfsAccessMode> modes) {
    config_->nfsUidAccessModes.setValue(
        std::move(modes), ConfigSourceType::UserConfig, true);
  }

  void setGidModes(std::unordered_map<uint32_t, NfsAccessMode> modes) {
    config_->nfsGidAccessModes.setValue(
        std::move(modes), ConfigSourceType::UserConfig, true);
  }

  void setRateLimit(uint32_t count, uint32_t windowSeconds) {
    config_->nfsAccessRateLimitCount.setValue(
        count, ConfigSourceType::UserConfig, true);
    config_->nfsAccessRateLimitWindowSeconds.setValue(
        windowSeconds, ConfigSourceType::UserConfig, true);
  }

  // uid 0 with gid 20 / aux {20}: matches a uid 0 entry only.
  static opaque_auth rootOnlyCred() {
    return makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/0, /*gid=*/20, {20}});
  }

  // uid 501 with primary gid 0: matches a gid 0 entry only.
  static opaque_auth wheelCred() {
    return makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/0, {0}});
  }

  // uid 501 with gid 0 only in the auxiliary list.
  static opaque_auth auxWheelCred() {
    return makeAuthSysCred(
        {/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/20, {20, 0}});
  }

  // uid 0 and gid 0: matches both default entries.
  static opaque_auth rootAndWheelCred() {
    return makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/0, /*gid=*/0, {0}});
  }

  // uid 501, gid 20: matches nothing in the default config.
  static opaque_auth userCred() {
    return makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/20, {20}});
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

  folly::EventBase evb_;
  std::shared_ptr<folly::ManualExecutor> manualExecutor_;
  std::shared_ptr<EdenConfig> config_;
  std::shared_ptr<ReloadableConfig> reloadableConfig_;
  std::unique_ptr<ErrorLogger> errorLogger_;
  folly::Logger straceLogger_{"eden.test.nfsd3"};
  UnixClock clock_;
  FakeNfsDispatcher* dispatcher_ = nullptr;
  std::unique_ptr<Nfsd3, FsChannelDeleter> nfsd3_;
  int clientFd_ = -1;
};

TEST_F(Nfsd3Test, default_config_counts_uid0_and_gid0) {
  auto uidBefore = getCounter("nfs.access.uid.0.sum");
  auto gidBefore = getCounter("nfs.access.gid.0.sum");

  expectAcceptedSuccess(sendGetattr(1, rootAndWheelCred()));
  EXPECT_EQ(getCounter("nfs.access.uid.0.sum") - uidBefore, 1);
  EXPECT_EQ(getCounter("nfs.access.gid.0.sum") - gidBefore, 1);

  // A plain user matches neither default entry.
  expectAcceptedSuccess(sendGetattr(2, userCred()));
  EXPECT_EQ(getCounter("nfs.access.uid.0.sum") - uidBefore, 1);
  EXPECT_EQ(getCounter("nfs.access.gid.0.sum") - gidBefore, 1);
}

TEST_F(Nfsd3Test, gid_entry_matches_auxiliary_gid) {
  auto gidBefore = getCounter("nfs.access.gid.0.sum");

  expectAcceptedSuccess(sendGetattr(1, auxWheelCred()));
  EXPECT_EQ(getCounter("nfs.access.gid.0.sum") - gidBefore, 1);

  // gid 20 / aux {20} carries no gid 0 anywhere.
  expectAcceptedSuccess(sendGetattr(2, rootOnlyCred()));
  EXPECT_EQ(getCounter("nfs.access.gid.0.sum") - gidBefore, 1);
}

TEST_F(Nfsd3Test, empty_maps_skip_everything) {
  setUidModes({});
  setGidModes({});
  auto uidBefore = getCounter("nfs.access.uid.0.sum");
  auto gidBefore = getCounter("nfs.access.gid.0.sum");
  auto blockedBefore = getCounter("nfs.blocked_access.sum");

  expectAcceptedSuccess(sendGetattr(1, rootAndWheelCred()));

  EXPECT_EQ(getCounter("nfs.access.uid.0.sum"), uidBefore);
  EXPECT_EQ(getCounter("nfs.access.gid.0.sum"), gidBefore);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum"), blockedBefore);
}

TEST_F(Nfsd3Test, block_entry_rejects_across_procedures) {
  setUidModes({{0, NfsAccessMode::Block}});
  auto accessBefore = getCounter("nfs.access.uid.0.sum");
  auto blockedUidBefore = getCounter("nfs.blocked.uid.0.sum");
  auto blockedBefore = getCounter("nfs.blocked_access.sum");

  expectAuthTooWeak(sendGetattr(1, rootOnlyCred()));
  expectAuthTooWeak(sendAccess(2, rootOnlyCred()));
  expectAuthTooWeak(sendRead(3, rootOnlyCred()));
  // The gid map still holds the default "0:log", so wheel is untouched.
  expectAcceptedSuccess(sendGetattr(4, wheelCred()));
  expectAcceptedSuccess(sendGetattr(5, userCred()));

  // "block" is a strict superset of "log": rejected requests are counted.
  EXPECT_EQ(getCounter("nfs.access.uid.0.sum") - accessBefore, 3);
  EXPECT_EQ(getCounter("nfs.blocked.uid.0.sum") - blockedUidBefore, 3);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum") - blockedBefore, 3);
}

TEST_F(Nfsd3Test, uid_and_gid_entries_are_independent) {
  setUidModes({});
  setGidModes({{0, NfsAccessMode::Block}});
  auto uidBefore = getCounter("nfs.access.uid.0.sum");

  // Both gid 0 spellings are rejected by the gid entry alone.
  expectAuthTooWeak(sendGetattr(1, wheelCred()));
  expectAuthTooWeak(sendGetattr(2, auxWheelCred()));
  // uid 0 has no entry: neither rejected nor counted.
  expectAcceptedSuccess(sendGetattr(3, rootOnlyCred()));

  EXPECT_EQ(getCounter("nfs.access.uid.0.sum"), uidBefore);
}

TEST_F(Nfsd3Test, arbitrary_ids_get_their_own_entries) {
  setUidModes({{501, NfsAccessMode::Block}});
  setGidModes({{20, NfsAccessMode::Log}});
  auto uidAccessBefore = getCounter("nfs.access.uid.501.sum");
  auto uidBlockedBefore = getCounter("nfs.blocked.uid.501.sum");
  auto gidAccessBefore = getCounter("nfs.access.gid.20.sum");
  auto gidBlockedBefore = getCounter("nfs.blocked.gid.20.sum");
  auto blockedBefore = getCounter("nfs.blocked_access.sum");

  expectAuthTooWeak(sendGetattr(1, userCred()));

  // Both entries are evaluated and counted; only the uid one rejects, and
  // the aggregate counter moves once per request, not once per entry.
  EXPECT_EQ(getCounter("nfs.access.uid.501.sum") - uidAccessBefore, 1);
  EXPECT_EQ(getCounter("nfs.blocked.uid.501.sum") - uidBlockedBefore, 1);
  EXPECT_EQ(getCounter("nfs.access.gid.20.sum") - gidAccessBefore, 1);
  EXPECT_EQ(getCounter("nfs.blocked.gid.20.sum"), gidBlockedBefore);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum") - blockedBefore, 1);
}

TEST_F(Nfsd3Test, every_matching_gid_entry_is_evaluated) {
  setUidModes({});
  setGidModes({{0, NfsAccessMode::Log}, {20, NfsAccessMode::Block}});
  auto gid0AccessBefore = getCounter("nfs.access.gid.0.sum");
  auto gid0BlockedBefore = getCounter("nfs.blocked.gid.0.sum");
  auto gid20AccessBefore = getCounter("nfs.access.gid.20.sum");
  auto gid20BlockedBefore = getCounter("nfs.blocked.gid.20.sum");
  auto blockedBefore = getCounter("nfs.blocked_access.sum");

  // Primary gid 20 and auxiliary gid 0 both match; the gid loop keeps going
  // past the first hit, so both entries are counted and the block one rejects.
  expectAuthTooWeak(sendGetattr(
      1, makeAuthSysCred({/*stamp=*/1, "mac", /*uid=*/501, /*gid=*/20, {0}})));

  EXPECT_EQ(getCounter("nfs.access.gid.0.sum") - gid0AccessBefore, 1);
  EXPECT_EQ(getCounter("nfs.access.gid.20.sum") - gid20AccessBefore, 1);
  EXPECT_EQ(getCounter("nfs.blocked.gid.20.sum") - gid20BlockedBefore, 1);
  EXPECT_EQ(getCounter("nfs.blocked.gid.0.sum"), gid0BlockedBefore);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum") - blockedBefore, 1);
}

TEST_F(Nfsd3Test, missing_creds_are_never_blocked) {
  setUidModes({{0, NfsAccessMode::Block}});
  setGidModes({{0, NfsAccessMode::Block}});

  expectAcceptedSuccess(
      sendGetattr(1, opaque_auth{auth_flavor::AUTH_NONE, {}}));
}

TEST_F(Nfsd3Test, control_plane_procs_are_exempt) {
  setUidModes({{0, NfsAccessMode::Block}});
  setGidModes({{0, NfsAccessMode::Block}});
  auto uidBefore = getCounter("nfs.access.uid.0.sum");
  auto blockedBefore = getCounter("nfs.blocked_access.sum");

  // Mount bookkeeping keeps working for a blocked identity...
  expectAcceptedSuccess(
      sendAndReceive(buildNfsRequest(1, nfsv3Procs::null, rootAndWheelCred())));
  expectAcceptedSuccess(sendAndReceive(buildNfsRequest(
      2,
      nfsv3Procs::fsstat,
      rootAndWheelCred(),
      FSSTAT3args{nfs_fh3{InodeNumber{42}}})));
  expectAcceptedSuccess(sendAndReceive(buildNfsRequest(
      3,
      nfsv3Procs::fsinfo,
      rootAndWheelCred(),
      FSINFO3args{nfs_fh3{InodeNumber{42}}})));
  expectAcceptedSuccess(sendAndReceive(buildNfsRequest(
      4,
      nfsv3Procs::pathconf,
      rootAndWheelCred(),
      PATHCONF3args{nfs_fh3{InodeNumber{42}}})));

  // ...and is neither counted nor blocked...
  EXPECT_EQ(getCounter("nfs.access.uid.0.sum"), uidBefore);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum"), blockedBefore);

  // ...while file access is still rejected.
  expectAuthTooWeak(sendGetattr(5, rootAndWheelCred()));
}

TEST_F(Nfsd3Test, rate_limit_allows_baseline_and_rejects_bursts) {
  setUidModes({{0, NfsAccessMode::RateLimit}});
  // A generous window so the budget cannot refill mid-test.
  setRateLimit(/*count=*/3, /*windowSeconds=*/3600);
  auto accessBefore = getCounter("nfs.access.uid.0.sum");
  auto blockedUidBefore = getCounter("nfs.blocked.uid.0.sum");
  auto blockedBefore = getCounter("nfs.blocked_access.sum");

  // The first `count` accesses in the window pass...
  expectAcceptedSuccess(sendGetattr(1, rootOnlyCred()));
  expectAcceptedSuccess(sendGetattr(2, rootOnlyCred()));
  expectAcceptedSuccess(sendGetattr(3, rootOnlyCred()));
  // ...and the burst beyond the budget is rejected like "block".
  expectAuthTooWeak(sendGetattr(4, rootOnlyCred()));
  expectAuthTooWeak(sendGetattr(5, rootOnlyCred()));

  // Every access is counted, allowed or not.
  EXPECT_EQ(getCounter("nfs.access.uid.0.sum") - accessBefore, 5);
  EXPECT_EQ(getCounter("nfs.blocked.uid.0.sum") - blockedUidBefore, 2);
  EXPECT_EQ(getCounter("nfs.blocked_access.sum") - blockedBefore, 2);
}

TEST_F(Nfsd3Test, rate_limit_budgets_are_per_id) {
  setUidModes({{0, NfsAccessMode::RateLimit}, {501, NfsAccessMode::RateLimit}});
  setRateLimit(/*count=*/1, /*windowSeconds=*/3600);

  // Exhausting uid 0's budget leaves uid 501's untouched.
  expectAcceptedSuccess(sendGetattr(1, rootOnlyCred()));
  expectAuthTooWeak(sendGetattr(2, rootOnlyCred()));
  expectAcceptedSuccess(sendGetattr(3, userCred()));
  expectAuthTooWeak(sendGetattr(4, userCred()));
}

TEST_F(Nfsd3Test, config_changes_apply_without_restart) {
  setUidModes({{0, NfsAccessMode::Block}});
  expectAuthTooWeak(sendGetattr(1, rootOnlyCred()));

  // Dropping the entry back to "log" unblocks the same running server.
  setUidModes({{0, NfsAccessMode::Log}});
  expectAcceptedSuccess(sendGetattr(2, rootOnlyCred()));

  // The gid map is picked up independently.
  setGidModes({{0, NfsAccessMode::Block}});
  expectAuthTooWeak(sendGetattr(3, rootAndWheelCred()));
  expectAcceptedSuccess(sendGetattr(4, rootOnlyCred()));
}

TEST(NfsAccessRateLimiterTest, budget_refills_over_time) {
  NfsAccessRateLimiter limiter;

  // Burst capacity: `count` accesses pass at one instant, further ones are
  // rejected. (The token bucket accrues from time zero, so start the
  // synthetic clock late enough for a full initial budget.)
  EXPECT_TRUE(
      limiter.allow(/*count=*/2, /*windowSeconds=*/60, /*nowSeconds=*/1000.0));
  EXPECT_TRUE(limiter.allow(2, 60, 1000.0));
  EXPECT_FALSE(limiter.allow(2, 60, 1000.0));

  // The budget refills continuously at count/window: one window later the
  // full burst is available again.
  EXPECT_TRUE(limiter.allow(2, 60, 1060.0));
  EXPECT_TRUE(limiter.allow(2, 60, 1060.0));
  EXPECT_FALSE(limiter.allow(2, 60, 1060.0));

  // Degenerate configs: zero count admits nothing, zero window admits
  // everything.
  EXPECT_FALSE(limiter.allow(0, 60, 1120.0));
  EXPECT_TRUE(limiter.allow(2, 0, 1120.0));
}

} // namespace

#endif
