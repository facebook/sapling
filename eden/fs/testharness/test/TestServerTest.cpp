/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/io/async/EventBase.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>

#include "eden/fs/service/EdenServer.h"
#include "eden/fs/testharness/TestServer.h"

#include <folly/portability/GTest.h>

using namespace facebook::eden;

namespace {
class TestServerTest : public ::testing::Test {
 protected:
  EdenServer& getServer() {
    return testServer_.getServer();
  }

  void runServer() {
    auto& thriftServer = getServer().getServer();
    thriftServer->serve();
  }

  template <typename F>
  void runOnServerStart(F&& fn) {
    auto cb = std::make_unique<folly::EventBase::FunctionLoopCallback>(
        std::forward<F>(fn));
    folly::EventBaseManager::get()->getEventBase()->runInLoop(cb.release());
  }

  TestServer testServer_;
};
} // namespace

TEST_F(TestServerTest, returnsVersionNumber) {
  runOnServerStart([&] {
    EXPECT_EQ(getServer().getVersion(), "test server");
    getServer().stop();
  });

  // Run the server.
  runServer();
}
