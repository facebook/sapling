/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServer.h"

#include <folly/CancellationToken.h>
#include <gtest/gtest.h>

#include "eden/fs/service/EdenServiceHandler.h"
#include "eden/fs/testharness/TestServer.h"

namespace facebook::eden {

class EdenServerTest : public ::testing::Test {
 protected:
  TestServer testServer_;
};

TEST_F(EdenServerTest, StopCancelsAllActiveRequests) {
  auto& server = testServer_.getServer();
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
  auto& server = testServer_.getServer();

  // Multiple calls to stop should not crash
  server.stop();
  server.stop();
  server.stop();

  // Test passes if no crash occurs
  EXPECT_TRUE(true);
}

} // namespace facebook::eden
