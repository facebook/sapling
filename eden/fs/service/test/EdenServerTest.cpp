/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServer.h"

#include <csignal>
#include <future>
#include <memory>
#include <system_error>
#include <thread>

#include <folly/CancellationToken.h>
#include <gflags/gflags.h>
#include <gtest/gtest.h>

#include "eden/common/utils/FaultInjector.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/service/EdenServiceHandler.h"
#include "eden/fs/service/EdenStateDir.h"
#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/testharness/TestServer.h"

using namespace std::chrono_literals;

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
  // The old daemon intentionally writes takeover data to a client that has
  // already disconnected. Ignore SIGPIPE so the broken send is reported as a
  // socket exception instead of killing the test process.
  signal(SIGPIPE, SIG_IGN);

  auto& server = testServer().getServer();
  ScopedServerThread serverThread{server};

  // Wait until the thrift server is fully initialized before attempting
  // takeover, otherwise the test races startup instead of the handoff path.
  testServer().waitUntilReady();

  auto& faultInjector = server.getServerState()->getFaultInjector();
  // Pause the old daemon after it sends the readiness ping. This lets the new
  // process reply and then disconnect before the takeover payload is sent.
  faultInjector.injectBlock("takeover", "ping_receive");

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
  EXPECT_FALSE(server.performCleanup());
}
#endif

} // namespace facebook::eden
