/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenStateDir.h"

#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"

using namespace facebook::eden;

namespace {

class EdenStateDirTest : public ::testing::Test {
 protected:
  AbsolutePath stateDirPath() const {
    return canonicalPath(tmpDir_.path().string());
  }

  folly::test::TemporaryDirectory tmpDir_;
};

TEST_F(EdenStateDirTest, restartSentinelIsNamedForItsPidAndToken) {
  const EdenStateDir stateDir{stateDirPath()};

  const auto path = stateDir.getRestartSentinelPath(1234, 0xdeadbeef);

  EXPECT_EQ(
      ".edenfs_restart_armed.1234.00000000deadbeef", path.basename().view());
  EXPECT_EQ(stateDirPath().asString(), path.dirname().asString());
}

TEST_F(EdenStateDirTest, restartSentinelNamesStartWithThePrefix) {
  const EdenStateDir stateDir{stateDirPath()};
  const auto prefix = stateDir.getRestartSentinelNamePrefix();
  const auto path = stateDir.getRestartSentinelPath(1, 2);
  const auto name = path.basename().view();

  EXPECT_EQ('.', prefix.back());
  EXPECT_TRUE(name.starts_with(prefix)) << name;
}

} // namespace
