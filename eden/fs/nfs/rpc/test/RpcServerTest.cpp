/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/rpc/RpcServer.h"

#include <folly/portability/GTest.h>
#include "eden/fs/telemetry/NullStructuredLogger.h"

namespace {

using namespace facebook::eden;

class TestServerProcessor : public RpcServerProcessor {};

struct RpcServerTest : ::testing::Test {
  std::shared_ptr<RpcServer> createServer() {
    return RpcServer::create(
        std::make_shared<TestServerProcessor>(),
        &evb,
        folly::getUnsafeMutableGlobalCPUExecutor(),
        std::make_shared<NullStructuredLogger>());
  }

  folly::EventBase evb;
};

TEST_F(RpcServerTest, takeover_before_initialize) {
  auto server = createServer();

  auto takeover = server->takeoverStop();
  evb.drive();
  EXPECT_TRUE(takeover.isReady());
}

TEST_F(RpcServerTest, takeover_after_initialize) {
  auto server = createServer();

  folly::SocketAddress addr;
  addr.setFromIpPort("::0", 0);
  server->initialize(addr);

  auto takeover = server->takeoverStop();
  evb.drive();
  EXPECT_TRUE(takeover.isReady());
}

TEST_F(RpcServerTest, takeover_from_takeover) {
  auto server = createServer();

  folly::SocketAddress addr;
  addr.setFromIpPort("::0", 0);
  server->initialize(addr);

  auto takeover = server->takeoverStop();
  evb.drive();
  EXPECT_TRUE(takeover.isReady());

  server.reset();
  evb.drive();

  auto newServer = createServer();
  newServer->initializeServerSocket(std::move(takeover).get());

  takeover = newServer->takeoverStop();
  evb.drive();
  EXPECT_TRUE(takeover.isReady());
}

} // namespace
