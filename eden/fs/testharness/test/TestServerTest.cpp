/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/TestServer.h"
#include "eden/fs/service/EdenServer.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(TestServerTest, returnsVersionNumber) {
  TestServer test;
  auto& server = test.getServer();
  EXPECT_EQ(server.getVersion(), "test server");
}
