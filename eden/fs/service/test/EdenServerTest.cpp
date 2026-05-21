/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServer.h"

#ifndef _WIN32
#include <poll.h>
#include <sys/socket.h>
#include <unistd.h>
#endif

#include <chrono>
#include <csignal>
#include <cstring>
#include <functional>
#include <future>
#include <memory>
#include <system_error>
#include <thread>

#include <folly/CancellationToken.h>
#include <folly/ExceptionWrapper.h>
#include <folly/ScopeGuard.h>
#include <folly/io/IOBufQueue.h>
#include <gflags/gflags.h>
#include <gtest/gtest.h>

#include "eden/common/utils/FaultInjector.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/nfs/MountdRpc.h"
#include "eden/fs/nfs/NfsServer.h"
#include "eden/fs/service/EdenServiceHandler.h"
#include "eden/fs/service/EdenStateDir.h"
#include "eden/fs/takeover/TakeoverClient.h"
#ifdef __linux__
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeFuse.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"
#endif
#include "eden/fs/testharness/TestServer.h"

using namespace std::chrono_literals;
#ifdef __linux__
using namespace folly::string_piece_literals;
#endif

namespace facebook::eden {

namespace {

#ifndef _WIN32
class ScopedServerThread {
 public:
  explicit ScopedServerThread(EdenServer& server)
      : server_{server},
        exitFuture_{exitPromise_.get_future().share()},
        thread_{[this] {
          try {
            server_.serve();
            exitPromise_.set_value();
          } catch (...) {
            exitPromise_.set_exception(std::current_exception());
          }
        }} {}

  ~ScopedServerThread() {
    server_.stop();
    join();
  }

  bool waitForExit(std::chrono::milliseconds timeout) const {
    return exitFuture_.wait_for(timeout) == std::future_status::ready;
  }

  void join() {
    if (thread_.joinable()) {
      thread_.join();
    }
  }

  void throwIfServeFailed() const {
    exitFuture_.get();
  }

 private:
  EdenServer& server_;
  std::promise<void> exitPromise_;
  std::shared_future<void> exitFuture_;
  std::thread thread_;
};

std::future<TakeoverData> takeoverViaThread(
    AbsolutePathPiece socketPath,
    bool shouldThrowDuringTakeover) {
  return std::async(
      std::launch::async,
      [path = AbsolutePath{socketPath}, shouldThrowDuringTakeover] {
        return takeoverMounts(
            path,
            /*takeoverReceiveTimeout=*/std::chrono::seconds{150},
            shouldThrowDuringTakeover);
      });
}

void driveTakeoverSendFailureToCleanup(
    TestServer& testServer,
    EdenServer& server,
    size_t blockCount = 0) {
  // The old daemon intentionally writes takeover data to a client that has
  // already disconnected. Ignore SIGPIPE so the broken send is reported as a
  // socket exception instead of killing the test process.
  signal(SIGPIPE, SIG_IGN);

  ScopedServerThread serverThread{server};

  // Wait until the thrift server is fully initialized before attempting
  // takeover, otherwise the test races startup instead of the handoff path.
  testServer.waitUntilReady();

  auto& faultInjector = server.getServerState()->getFaultInjector();
  // Pause the old daemon after it sends the readiness ping. This lets the new
  // process reply and then disconnect before the takeover payload is sent.
  faultInjector.injectBlock("takeover", "ping_receive", blockCount);

  auto socketPath = EdenStateDir{server.getEdenDir()}.getTakeoverSocketPath();
  auto clientFuture = takeoverViaThread(
      socketPath,
      /*shouldThrowDuringTakeover=*/true);

  // Drive the server event loop until the takeover path reaches the injected
  // stall, which means mounts have already been stopped for handoff.
  bool takeoverBlocked = false;
  std::thread blockedWaiter([&] {
    takeoverBlocked = faultInjector.waitUntilBlocked("takeover", 5s);
    server.getMainEventBase()->runInEventBaseThread(
        [evb = server.getMainEventBase()] { evb->terminateLoopSoon(); });
  });
  server.getMainEventBase()->loop();
  blockedWaiter.join();

  ASSERT_TRUE(takeoverBlocked);

  // The new process now exits before takeover data transfer begins.
  bool clientSawException = false;
  try {
    (void)clientFuture.get();
  } catch (const std::exception&) {
    clientSawException = true;
  }
  ASSERT_TRUE(clientSawException);

  // Let the old daemon continue into the send path, then wait for the serve
  // loop to exit before calling performCleanup().
  faultInjector.removeFault("takeover", "ping_receive");
  faultInjector.unblock("takeover", "ping_receive");

  ASSERT_TRUE(serverThread.waitForExit(5s));
  serverThread.join();
  ASSERT_NO_THROW(serverThread.throwIfServeFailed());
}

std::unique_ptr<folly::IOBuf> buildMountdNullRpcRequest(uint32_t xid) {
  folly::IOBufQueue queue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender ser(&queue, 256);

  XdrTrait<uint32_t>::serialize(ser, 0); // fragment header placeholder
  rpc_msg_call call{
      xid,
      msg_type::CALL,
      call_body{
          kRPCVersion,
          kMountdProgNumber,
          kMountdProgVersion,
          static_cast<uint32_t>(mountProcs::null),
          opaque_auth{auth_flavor::AUTH_NONE, {}},
          opaque_auth{auth_flavor::AUTH_NONE, {}},
      },
  };
  XdrTrait<rpc_msg_call>::serialize(ser, call);

  auto len = static_cast<uint32_t>(queue.chainLength() - sizeof(uint32_t));
  auto buf = queue.move();
  const uint32_t fragmentHeader = htonl(len | 0x80000000);
  std::memcpy(buf->writableData(), &fragmentHeader, sizeof(fragmentHeader));
  return buf;
}

int connectSocket(const folly::SocketAddress& addr) {
  sockaddr_storage socketAddress{};
  auto len = addr.getAddress(&socketAddress);

  int fd = socket(addr.getFamily(), SOCK_STREAM, 0);
  if (fd == -1) {
    throw std::system_error(
        errno,
        std::generic_category(),
        "failed to create mountd client socket");
  }

  if (connect(fd, reinterpret_cast<const sockaddr*>(&socketAddress), len) !=
      0) {
    auto savedErrno = errno;
    close(fd);
    throw std::system_error(
        savedErrno, std::generic_category(), "failed to connect to mountd");
  }

  return fd;
}

bool pollForReply(int clientFd, int timeoutMs) {
  struct pollfd pfd{};
  pfd.fd = clientFd;
  pfd.events = POLLIN;
  return poll(&pfd, 1, timeoutMs) > 0 && (pfd.revents & POLLIN);
}

template <typename Predicate>
bool driveMainEventBaseUntil(
    EdenServer& server,
    Predicate&& predicate,
    std::chrono::milliseconds timeout = std::chrono::seconds{5}) {
  auto* evb = server.getMainEventBase();
  struct PollState : std::enable_shared_from_this<PollState> {
    PollState(
        folly::EventBase* evb,
        std::function<bool()> predicate,
        std::chrono::steady_clock::time_point deadline,
        std::chrono::milliseconds pollInterval)
        : evb{evb},
          predicate{std::move(predicate)},
          deadline{deadline},
          pollInterval{pollInterval} {}

    void check() {
      if (finished) {
        return;
      }
      if (predicate()) {
        reachedCondition = true;
        evb->terminateLoopSoon();
        return;
      }
      if (std::chrono::steady_clock::now() >= deadline) {
        evb->terminateLoopSoon();
        return;
      }
      auto state = this->shared_from_this();
      evb->runAfterDelay(
          [state] { state->check(); },
          static_cast<uint32_t>(pollInterval.count()));
    }

    folly::EventBase* evb;
    std::function<bool()> predicate;
    std::chrono::steady_clock::time_point deadline;
    std::chrono::milliseconds pollInterval;
    bool finished{false};
    bool reachedCondition{false};
  };

  constexpr auto kPollInterval = 10ms;
  auto state = std::make_shared<PollState>(
      evb,
      std::forward<Predicate>(predicate),
      std::chrono::steady_clock::now() + timeout,
      kPollInterval);
  evb->runInEventBaseThread([state] { state->check(); });
  evb->loop();
  state->finished = true;
  return state->reachedCondition;
}
#endif

} // namespace

class EdenServerTest : public ::testing::Test {
 protected:
  EdenServerTest() {
    GFLAGS_NAMESPACE::SetCommandLineOptionWithMode(
        "enable_fault_injection", "true", GFLAGS_NAMESPACE::SET_FLAGS_VALUE);
    testServer_ = std::make_unique<TestServer>();
  }

  TestServer& testServer() {
    return *testServer_;
  }

 private:
  std::unique_ptr<TestServer> testServer_;
};

TEST_F(EdenServerTest, StopCancelsAllActiveRequests) {
  auto& server = testServer().getServer();
  auto handler = server.getHandler();

  // Simulate active Thrift request
  folly::CancellationSource source;
  auto token = source.getToken();

  bool requestCancelled = false;
  folly::CancellationCallback callback(
      token, [&requestCancelled] { requestCancelled = true; });

  uint64_t testRequestId = 12345;
  handler->insertCancellationSource(
      testRequestId, std::move(source), "test_endpoint");

  EXPECT_FALSE(requestCancelled);
  EXPECT_EQ(handler->getActiveCancellationSourceCount(), 1);

  // Trigger server stop & cancel all active requests
  server.stop();

  // Active requests are now cancelled
  EXPECT_TRUE(requestCancelled);
  EXPECT_TRUE(token.isCancellationRequested());
}

TEST_F(EdenServerTest, StopIsIdempotent) {
  auto& server = testServer().getServer();

  // Multiple calls to stop should not crash
  server.stop();
  server.stop();
  server.stop();

  // Test passes if no crash occurs
  EXPECT_TRUE(true);
}

#ifndef _WIN32
TEST_F(EdenServerTest, TakeoverSendFailureRecoversDuringCleanup) {
  auto& server = testServer().getServer();
  ASSERT_NO_FATAL_FAILURE(
      driveTakeoverSendFailureToCleanup(testServer(), server));
  EXPECT_FALSE(server.performCleanup());
}

TEST_F(EdenServerTest, TakeoverSendFailureRecoveryReinitializesMountd) {
  TestServer::Options nfsOptions;
  nfsOptions.enableNfsServer = true;
  TestServer nfsTestServer{nfsOptions};
  auto& server = nfsTestServer.getServer();

  ASSERT_NO_FATAL_FAILURE(
      driveTakeoverSendFailureToCleanup(nfsTestServer, server));
  ASSERT_FALSE(server.performCleanup());

  std::thread recoveryEventBaseThread(
      [&server] { server.getMainEventBase()->loop(); });
  auto stopRecoveryLoop = folly::makeGuard([&] {
    server.getMainEventBase()->runInEventBaseThread(
        [&server] { server.getMainEventBase()->terminateLoopSoon(); });
    if (recoveryEventBaseThread.joinable()) {
      recoveryEventBaseThread.join();
    }
  });

  auto nfsServer = server.getServerState()->getNfsServer();
  ASSERT_TRUE(nfsServer);
  auto request = buildMountdNullRpcRequest(42);
  const auto requestBytes = request->coalesce();
  const int mountdFd = connectSocket(nfsServer->getMountdAddr());
  auto closeMountdFd = folly::makeGuard([mountdFd] { close(mountdFd); });

  ASSERT_EQ(
      static_cast<ssize_t>(requestBytes.size()),
      write(mountdFd, requestBytes.data(), requestBytes.size()));
  const auto hasReply = pollForReply(mountdFd, 1000);
  EXPECT_TRUE(hasReply)
      << "mountd should accept null RPCs after takeover recovery";

  if (hasReply) {
    uint8_t reply[256];
    EXPECT_GT(read(mountdFd, reply, sizeof(reply)), 0);
  }
}

TEST_F(EdenServerTest, RepeatedTakeoverFailuresDoNotBreakShutdownFuture) {
  auto& server = testServer().getServer();
  auto& faultInjector = server.getServerState()->getFaultInjector();
  auto socketPath = EdenStateDir{server.getEdenDir()}.getTakeoverSocketPath();

  {
    // First reproduce the original handoff failure: the new process replies to
    // the readiness ping and then disconnects before takeover data transfer
    // completes. The daemon should recover and continue serving.
    ASSERT_NO_FATAL_FAILURE(
        driveTakeoverSendFailureToCleanup(testServer(), server, 1));
    EXPECT_FALSE(server.performCleanup());
  }

  {
    // Then trigger a second takeover failure after the server has already
    // recovered once. This exercises the path where TakeoverData has already
    // been prepared and must not leave takeoverComplete as a broken promise.
    ScopedServerThread serverThread{server};
    ASSERT_TRUE(driveMainEventBaseUntil(server, [&] {
      return server.getStatus() == EdenServer::RunState::RUNNING;
    }));

    faultInjector.injectError(
        "takeover",
        "post_prepare_data",
        folly::make_exception_wrapper<std::runtime_error>(
            "simulated takeover failure after data preparation"),
        1);

    auto clientFuture = takeoverViaThread(
        socketPath,
        /*shouldThrowDuringTakeover=*/false);

    ASSERT_TRUE(driveMainEventBaseUntil(server, [&] {
      return clientFuture.wait_for(0ms) == std::future_status::ready;
    }));

    bool clientSawException = false;
    try {
      (void)clientFuture.get();
    } catch (const std::exception&) {
      clientSawException = true;
    }
    ASSERT_TRUE(clientSawException);

    ASSERT_TRUE(serverThread.waitForExit(5s));
    serverThread.join();
    ASSERT_NO_THROW(serverThread.throwIfServeFailed());
    EXPECT_FALSE(server.performCleanup());
  }

  {
    // Finally verify the daemon is still healthy enough to restart cleanly
    // after both failed takeover attempts.
    ScopedServerThread serverThread{server};
    ASSERT_TRUE(driveMainEventBaseUntil(server, [&] {
      return server.getStatus() == EdenServer::RunState::RUNNING;
    }));

    server.stop();
    ASSERT_TRUE(serverThread.waitForExit(5s));
    serverThread.join();
    ASSERT_NO_THROW(serverThread.throwIfServeFailed());
    EXPECT_TRUE(server.performCleanup());
  }
}
#endif

TEST_F(EdenServerTest, StopAllGarbageCollectionsDoesNotPoisonFutureGC) {
  auto& server = testServer().getServer();
#ifdef __linux__
  FakeTreeBuilder builder;
  builder.setFile("hello", "world");
  TestMount mount{builder};
  mount.updateEdenConfig({{"experimental:enable-pressure-based-gc", "true"}});

  auto fuse = std::make_shared<FakeFuse>();
  mount.startFuseAndWait(fuse);

  auto entry =
      mount.getDispatcher()
          ->lookup(
              0, kRootNodeId, "hello"_pc, ObjectFetchContext::getNullContext())
          .get(0ms);
  ASSERT_NE(0, entry.nodeid);

  EXPECT_TRUE(server.stopAllGarbageCollections(
      /*maxRetries=*/0, /*retryInterval=*/std::chrono::seconds{0}));

  auto numInvalidated = server
                            .garbageCollectWorkingCopy(
                                *mount.getEdenMount(),
                                mount.getRootInode(),
                                std::chrono::system_clock::now() + 1h,
                                ObjectFetchContext::getNullContext(),
                                /*pressureBased=*/true)
                            .get(10s);
  EXPECT_GT(numInvalidated, 0u);

  mount.getEdenMount()->flushInvalidations().get(10s);
  fuse->close();
  mount.getEdenMount()->getFsChannelCompletionFuture().within(10s).getVia(
      mount.getServerExecutor().get());
#else
  EXPECT_TRUE(server.stopAllGarbageCollections(
      /*maxRetries=*/0, /*retryInterval=*/std::chrono::seconds{0}));
#endif
}

} // namespace facebook::eden
