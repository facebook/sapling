/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/rpc/RpcServer.h"

#ifndef _WIN32
#include <poll.h>
#include <sys/socket.h>
#include <unistd.h>
#endif

#include <cstring>

#include <folly/executors/ManualExecutor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/IOBufQueue.h>
#include <gtest/gtest.h>

#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/utils/RequestPermitVendor.h"

namespace {

using namespace facebook::eden;

class TestServerProcessor : public RpcServerProcessor {};

class TestFastPathProcessor : public RpcServerProcessor {
 public:
  bool shouldFastPathRPCs() const override {
    return true;
  }
  bool isUnimplementedProc(uint32_t proc) const override {
    return proc >= 22;
  }
};

std::unique_ptr<folly::IOBuf> buildRpcRequest(uint32_t xid, uint32_t proc) {
  folly::IOBufQueue queue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender ser(&queue, 256);

  XdrTrait<uint32_t>::serialize(ser, 0); // fragment header placeholder
  rpc_msg_call call{
      xid,
      msg_type::CALL,
      call_body{
          kRPCVersion,
          kNfsdProgNumber,
          kNfsd3ProgVersion,
          proc,
          opaque_auth{auth_flavor::AUTH_NONE, {}},
          opaque_auth{auth_flavor::AUTH_NONE, {}},
      },
  };
  XdrTrait<rpc_msg_call>::serialize(ser, call);

  auto len = static_cast<uint32_t>(queue.chainLength() - sizeof(uint32_t));
  auto buf = queue.move();
  auto* header = reinterpret_cast<uint32_t*>(buf->writableData());
  *header = folly::Endian::big(len | 0x80000000);
  return buf;
}

std::unique_ptr<folly::IOBuf> buildNullRpcRequest(uint32_t xid) {
  return buildRpcRequest(xid, 0);
}

struct RpcServerTest : ::testing::Test {
  std::shared_ptr<RpcServer> createTestServer(
      std::shared_ptr<RpcServerProcessor> proc,
      std::shared_ptr<folly::Executor> executor =
          folly::getUnsafeMutableGlobalCPUExecutor()) {
    return RpcServer::create(
        std::move(proc),
        &evb,
        std::move(executor),
        nullptr,
        /*maximumInFlightRequests=*/1000,
        /*highNfsRequestsLogInterval=*/std::chrono::minutes{10});
  }

  std::shared_ptr<RpcServer> createTestServerWithManualExecutor(
      std::shared_ptr<RpcServerProcessor> proc) {
    manualExecutor_ = std::make_shared<folly::ManualExecutor>();
    return createTestServer(std::move(proc), manualExecutor_);
  }

#ifndef _WIN32
  /**
   * Create a connected socketpair, initialize the server with one end,
   * and return the client fd. Caller must close the returned fd.
   */
  int connectClient(RpcServer& server) {
    int fds[2];
    EXPECT_EQ(0, socketpair(AF_UNIX, SOCK_STREAM, 0, fds));
    server.initializeConnectedSocket(folly::File(fds[0], true));
    return fds[1];
  }

  /**
   * Write a serialized RPC request to the client fd and drive the
   * EventBase so it reads the bytes and processes any inline fast-path
   * replies (null, PROC_UNAVAIL, JUKEBOX) before dispatching to the
   * thread pool.
   */
  void sendRequest(int clientFd, std::unique_ptr<folly::IOBuf> request) {
    auto bytes = request->coalesce();
    ASSERT_EQ(
        static_cast<ssize_t>(bytes.size()),
        write(clientFd, bytes.data(), bytes.size()));
    evb.loopOnce();
  }

  /**
   * Poll for data on the client fd. Returns true if data is available
   * within @p timeoutMs milliseconds. On a Unix socketpair the reply
   * is local and arrives in microseconds; callers use a generous
   * timeout as a safety net so a broken test fails with a clear
   * message instead of hanging the test runner.
   */
  bool pollForReply(int clientFd, int timeoutMs) {
    struct pollfd pfd{};
    pfd.fd = clientFd;
    pfd.events = POLLIN;
    return poll(&pfd, 1, timeoutMs) > 0;
  }

  /**
   * Read a reply from the client fd. Assumes a single read() returns
   * the complete reply — safe on Unix socketpairs with small messages
   * but would need a loop for TCP or replies larger than 256 bytes.
   */
  std::vector<uint8_t> readReply(int clientFd) {
    uint8_t buf[256];
    auto nread = read(clientFd, buf, sizeof(buf));
    EXPECT_GT(nread, 0);
    return std::vector<uint8_t>(buf, buf + nread);
  }

  /**
   * Send an RPC request and wait for the reply on the EventBase.
   * Returns the raw reply bytes. Asserts that a reply arrives within
   * the timeout.
   */
  std::vector<uint8_t> sendAndReceive(
      int clientFd,
      std::unique_ptr<folly::IOBuf> request,
      const char* failMsg = "Expected an RPC reply") {
    sendRequest(clientFd, std::move(request));
    EXPECT_TRUE(pollForReply(clientFd, 1000)) << failMsg;
    return readReply(clientFd);
  }

  /**
   * Read a big-endian uint32_t from a raw reply at the given byte offset.
   */
  static uint32_t readBigEndianU32(
      const std::vector<uint8_t>& data,
      size_t offset) {
    EXPECT_GE(data.size(), offset + 4);
    uint32_t val;
    memcpy(&val, data.data() + offset, sizeof(val));
    return folly::Endian::big(val);
  }

  /**
   * Clean up: close client fd, reset server, drain EventBase.
   */
  void cleanup(int clientFd, std::shared_ptr<RpcServer>& server) {
    close(clientFd);
    server.reset();
    evb.loopOnce();
  }
#endif // !_WIN32

  folly::EventBase evb;
  std::shared_ptr<folly::ManualExecutor> manualExecutor_;
};

TEST_F(RpcServerTest, takeover_before_initialize) {
  auto server = createTestServer(std::make_shared<TestServerProcessor>());

  auto takeover = server->takeoverStop();
  evb.drive();
  EXPECT_TRUE(takeover.isReady());
}

TEST_F(RpcServerTest, takeover_after_initialize) {
  auto server = createTestServer(std::make_shared<TestServerProcessor>());

  folly::SocketAddress addr;
  addr.setFromIpPort("::0", 0);
  server->initialize(addr);

  auto takeover = server->takeoverStop();
  evb.drive();
  EXPECT_TRUE(takeover.isReady());
}

TEST_F(RpcServerTest, takeover_from_takeover) {
  auto server = createTestServer(std::make_shared<TestServerProcessor>());

  folly::SocketAddress addr;
  addr.setFromIpPort("::0", 0);
  server->initialize(addr);

  auto takeover = server->takeoverStop();
  evb.drive();
  EXPECT_TRUE(takeover.isReady());

  server.reset();
  evb.drive();

  auto newServer = createTestServer(std::make_shared<TestServerProcessor>());
  newServer->initializeServerSocket(std::move(takeover).get());

  takeover = newServer->takeoverStop();
  evb.drive();
  EXPECT_TRUE(takeover.isReady());
}

#ifndef _WIN32
// Tests below use Unix socketpair/poll APIs not available on Windows.

TEST_F(RpcServerTest, null_rpc_bypasses_thread_pool) {
  auto server = createTestServerWithManualExecutor(
      std::make_shared<TestFastPathProcessor>());
  auto clientFd = connectClient(*server);

  auto reply = sendAndReceive(
      clientFd,
      buildNullRpcRequest(42),
      "Null RPC reply should arrive without needing the thread pool");

  EXPECT_EQ(readBigEndianU32(reply, 4), 42u); // xid
  EXPECT_EQ(readBigEndianU32(reply, 8), 1u); // msg_type::REPLY
  cleanup(clientFd, server);
}

TEST_F(RpcServerTest, proc_unavail_fast_path) {
  auto server = createTestServerWithManualExecutor(
      std::make_shared<TestFastPathProcessor>());
  auto clientFd = connectClient(*server);

  // Send an unknown proc (99) which isUnimplementedProc returns true for.
  auto reply = sendAndReceive(
      clientFd,
      buildRpcRequest(55, /*proc=*/99),
      "PROC_UNAVAIL reply should arrive without needing the thread pool");

  EXPECT_EQ(readBigEndianU32(reply, 4), 55u); // xid
  EXPECT_EQ(readBigEndianU32(reply, 8), 1u); // msg_type::REPLY
  // accept_stat::PROC_UNAVAIL = 3, at offset 24
  EXPECT_EQ(readBigEndianU32(reply, 24), 3u); // accept_stat::PROC_UNAVAIL
  cleanup(clientFd, server);
}

TEST_F(RpcServerTest, normal_proc_not_fast_pathed) {
  auto server = createTestServerWithManualExecutor(
      std::make_shared<TestFastPathProcessor>());
  auto clientFd = connectClient(*server);

  sendRequest(clientFd, buildRpcRequest(77, /*proc=*/1));

  // No inline reply — proc=1 is neither null nor unimplemented, so it
  // should be dispatched to the thread pool, not fast-pathed.
  EXPECT_FALSE(pollForReply(clientFd, 100))
      << "Normal proc should not get an inline reply";

  // The dispatch pipeline has multiple hops through the ManualExecutor
  // and EventBase. Alternate cranking both until the reply arrives.
  for (int i = 0; i < 20; ++i) {
    manualExecutor_->run();
    evb.loopOnce(EVLOOP_NONBLOCK);
  }

  EXPECT_TRUE(pollForReply(clientFd, 1000))
      << "Reply should arrive after cranking the thread pool";

  cleanup(clientFd, server);
}

class TestJukeboxProcessor : public RpcServerProcessor {
 public:
  bool shouldFastPathRPCs() const override {
    return true;
  }

  InlineRejectResult tryInlineReject() override {
    if (rejectAll_.load()) {
      return {true, nullptr};
    }
    return {};
  }

  void serializeInlineReject(
      uint32_t /*proc*/,
      uint32_t xid,
      folly::io::QueueAppender& ser) override {
    serializeReply(ser, accept_stat::SUCCESS, xid);
    GETATTR3res res;
    res.tag = nfsstat3::NFS3ERR_JUKEBOX;
    XdrTrait<GETATTR3res>::serialize(ser, res);
  }

  std::atomic<bool> rejectAll_{true};
};

class TestPermitProcessor : public RpcServerProcessor {
 public:
  explicit TestPermitProcessor(size_t capacity) : vendor_(capacity) {}

  bool shouldFastPathRPCs() const override {
    return true;
  }

  bool isUnimplementedProc(uint32_t proc) const override {
    return proc >= 22;
  }

  InlineRejectResult tryInlineReject() override {
    auto permit = vendor_.tryAcquirePermit();
    if (!permit) {
      return {true, nullptr};
    }
    return {false, std::move(permit)};
  }

  void serializeInlineReject(
      uint32_t /*proc*/,
      uint32_t xid,
      folly::io::QueueAppender& ser) override {
    serializeReply(ser, accept_stat::SUCCESS, xid);
    GETATTR3res res;
    res.tag = nfsstat3::NFS3ERR_JUKEBOX;
    XdrTrait<GETATTR3res>::serialize(ser, res);
  }

  RequestPermitVendor& vendor() {
    return vendor_;
  }

 private:
  RequestPermitVendor vendor_;
};

TEST_F(RpcServerTest, jukebox_rejects_non_exempt_inline) {
  auto server = createTestServerWithManualExecutor(
      std::make_shared<TestJukeboxProcessor>());
  auto clientFd = connectClient(*server);

  auto reply = sendAndReceive(
      clientFd,
      buildRpcRequest(100, /*proc=*/1),
      "JUKEBOX reply should arrive without the thread pool");

  EXPECT_EQ(readBigEndianU32(reply, 4), 100u);
  EXPECT_EQ(readBigEndianU32(reply, 8), 1u); // msg_type::REPLY
  EXPECT_EQ(readBigEndianU32(reply, 24), 0u); // accept_stat::SUCCESS

  // Verify the NFS-level JUKEBOX error in the response body.
  // Skip fragment header (4) + RPC reply envelope (24) = 28 bytes to reach
  // the NFS response. The first field is the nfsstat3 tag.
  ASSERT_GE(reply.size(), 32u) << "Reply too short for NFS status";
  uint32_t nfsStat = readBigEndianU32(reply, 28);
  EXPECT_EQ(nfsStat, static_cast<uint32_t>(nfsstat3::NFS3ERR_JUKEBOX));

  cleanup(clientFd, server);
}

TEST_F(RpcServerTest, jukebox_rejects_fsinfo_when_rate_limited) {
  auto server = createTestServerWithManualExecutor(
      std::make_shared<TestJukeboxProcessor>());
  auto clientFd = connectClient(*server);

  // FSINFO (proc=19) is now subject to JUKEBOX backpressure like any
  // other implemented proc.
  auto reply = sendAndReceive(
      clientFd,
      buildRpcRequest(200, /*proc=*/19),
      "FSINFO should be JUKEBOX-rejected when rate limited");

  EXPECT_EQ(readBigEndianU32(reply, 4), 200u);
  EXPECT_EQ(readBigEndianU32(reply, 8), 1u); // msg_type::REPLY
  EXPECT_EQ(readBigEndianU32(reply, 24), 0u); // accept_stat::SUCCESS

  // Verify the NFS-level JUKEBOX error in the response body.
  ASSERT_GE(reply.size(), 32u) << "Reply too short for NFS status";
  uint32_t nfsStat = readBigEndianU32(reply, 28);
  EXPECT_EQ(nfsStat, static_cast<uint32_t>(nfsstat3::NFS3ERR_JUKEBOX));

  cleanup(clientFd, server);
}

TEST_F(RpcServerTest, permit_exhausted_rejects_normal_proc) {
  auto proc = std::make_shared<TestPermitProcessor>(1);
  auto server = createTestServerWithManualExecutor(proc);
  auto clientFd = connectClient(*server);

  // Saturate the permit vendor — hold the only permit.
  auto held = proc->vendor().tryAcquirePermit();
  ASSERT_NE(held, nullptr);

  // Send a normal proc. With permits exhausted, it should be
  // JUKEBOX-rejected inline.
  auto reply = sendAndReceive(
      clientFd,
      buildRpcRequest(100, /*proc=*/1),
      "JUKEBOX reject should arrive inline when permits exhausted");

  EXPECT_EQ(readBigEndianU32(reply, 4), 100u); // xid
  EXPECT_EQ(readBigEndianU32(reply, 24), 0u); // accept_stat::SUCCESS
  ASSERT_GE(reply.size(), 32u);
  EXPECT_EQ(
      readBigEndianU32(reply, 28),
      static_cast<uint32_t>(nfsstat3::NFS3ERR_JUKEBOX));

  cleanup(clientFd, server);
}

TEST_F(RpcServerTest, permit_exhausted_still_fast_paths_null) {
  auto proc = std::make_shared<TestPermitProcessor>(1);
  auto server = createTestServerWithManualExecutor(proc);
  auto clientFd = connectClient(*server);

  // Saturate permits.
  auto held = proc->vendor().tryAcquirePermit();
  ASSERT_NE(held, nullptr);

  // Null RPCs bypass rate limiting — fast-pathed before the permit check.
  auto reply = sendAndReceive(
      clientFd,
      buildNullRpcRequest(101),
      "Null RPC should be fast-pathed even with permits exhausted");

  EXPECT_EQ(readBigEndianU32(reply, 4), 101u); // xid
  EXPECT_EQ(readBigEndianU32(reply, 8), 1u); // msg_type::REPLY
  EXPECT_EQ(readBigEndianU32(reply, 24), 0u); // accept_stat::SUCCESS
  // Reply should be short — just the RPC envelope, no NFS body.
  // Specifically, it should NOT contain NFS3ERR_JUKEBOX.
  EXPECT_LT(reply.size(), 32u)
      << "Null reply should be just the RPC envelope, not a JUKEBOX response";

  cleanup(clientFd, server);
}

TEST_F(RpcServerTest, permit_held_during_request_processing) {
  auto proc = std::make_shared<TestPermitProcessor>(1);
  auto server = createTestServerWithManualExecutor(proc);
  auto clientFd = connectClient(*server);

  // Send a normal proc. It should acquire the single permit and
  // dispatch to the ManualExecutor.
  sendRequest(clientFd, buildRpcRequest(200, /*proc=*/1));

  // The request is in the ManualExecutor queue. The permit should
  // still be held -- not released until the request completes.
  EXPECT_EQ(proc->vendor().available(), 0u)
      << "Permit should be held while request is in-flight";

  // A second request should be JUKEBOX-rejected because the permit
  // is held by the first request.
  auto reply = sendAndReceive(
      clientFd,
      buildRpcRequest(201, /*proc=*/1),
      "Second request should be JUKEBOX-rejected");

  EXPECT_EQ(readBigEndianU32(reply, 4), 201u);
  ASSERT_GE(reply.size(), 32u);
  EXPECT_EQ(
      readBigEndianU32(reply, 28),
      static_cast<uint32_t>(nfsstat3::NFS3ERR_JUKEBOX));

  // Crank the executor to complete the first request.
  for (int i = 0; i < 20; ++i) {
    manualExecutor_->run();
    evb.loopOnce(EVLOOP_NONBLOCK);
  }

  // Permit should now be released.
  EXPECT_EQ(proc->vendor().available(), 1u)
      << "Permit should be released after request completes";

  cleanup(clientFd, server);
}

TEST_F(RpcServerTest, permit_exhausted_still_fast_paths_unimplemented) {
  auto proc = std::make_shared<TestPermitProcessor>(1);
  auto server = createTestServerWithManualExecutor(proc);
  auto clientFd = connectClient(*server);

  // Saturate permits.
  auto held = proc->vendor().tryAcquirePermit();
  ASSERT_NE(held, nullptr);

  // Unimplemented procs bypass rate limiting — fast-pathed as PROC_UNAVAIL
  // before the permit check.
  auto reply = sendAndReceive(
      clientFd,
      buildRpcRequest(102, /*proc=*/99),
      "Unimplemented proc should be fast-pathed even with permits exhausted");

  EXPECT_EQ(readBigEndianU32(reply, 4), 102u); // xid
  EXPECT_EQ(readBigEndianU32(reply, 8), 1u); // msg_type::REPLY
  // accept_stat::PROC_UNAVAIL = 3
  EXPECT_EQ(readBigEndianU32(reply, 24), 3u);

  cleanup(clientFd, server);
}

class TestTimingProcessor : public RpcServerProcessor {
 public:
  bool shouldFastPathRPCs() const override {
    return false;
  }

  void onRequestComplete(const RpcRequestTimeline& t) override {
    lastTimeline_ = t;
    completedCount_++;
  }

  std::optional<RpcRequestTimeline> lastTimeline_;
  int completedCount_{0};
};

TEST_F(RpcServerTest, phase_timing_records_all_phases) {
  auto proc = std::make_shared<TestTimingProcessor>();
  auto server = createTestServerWithManualExecutor(proc);
  auto clientFd = connectClient(*server);

  // Send a null RPC through the full pipeline (shouldFastPathRPCs=false).
  sendRequest(clientFd, buildNullRpcRequest(99));

  // Crank the ManualExecutor and EventBase to complete the pipeline.
  for (int i = 0; i < 20; ++i) {
    manualExecutor_->run();
    evb.loopOnce(EVLOOP_NONBLOCK);
  }

  ASSERT_TRUE(pollForReply(clientFd, 1000))
      << "Reply should arrive after cranking the thread pool";
  readReply(clientFd);

  // Drive EventBase once more to ensure the WriteCallback has fired.
  evb.loopOnce(EVLOOP_NONBLOCK);

  // onRequestComplete should have been called exactly once.
  ASSERT_EQ(proc->completedCount_, 1);

  auto& t = *proc->lastTimeline_;

  // All five timestamps should be populated.
  ASSERT_TRUE(t.requestReceived.has_value()) << "requestReceived not set";
  ASSERT_TRUE(t.dispatched.has_value()) << "dispatched not set";
  ASSERT_TRUE(t.handlerStart.has_value()) << "handlerStart not set";
  ASSERT_TRUE(t.handlerDone.has_value()) << "handlerDone not set";
  ASSERT_TRUE(t.responseSent.has_value()) << "responseSent not set";

  // Timestamps should be in chronological order.
  EXPECT_LE(*t.requestReceived, *t.dispatched);
  EXPECT_LE(*t.dispatched, *t.handlerStart);
  EXPECT_LE(*t.handlerStart, *t.handlerDone);
  EXPECT_LE(*t.handlerDone, *t.responseSent);

  cleanup(clientFd, server);
}

#endif // !_WIN32

} // namespace
