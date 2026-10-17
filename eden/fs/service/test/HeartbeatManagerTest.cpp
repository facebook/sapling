/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/FileUtil.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>
#include <filesystem>
#include <memory>
#include <optional>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/test/OverlayTestUtil.h"
#include "eden/fs/service/EdenStateDir.h"
#include "eden/fs/service/HeartbeatManager.h"

using namespace facebook::eden;

TEST(HeartbeatManagerTest, emptyHeartbeatFileLeftByAnInterruptedWrite) {
  folly::test::TemporaryDirectory tmp;
  EdenStateDir stateDir{canonicalPath(tmp.path().string())};
  auto leftover = tmp.path() / "heartbeat_1.AbCdEf";
  ASSERT_TRUE(folly::writeFile(std::string{}, leftover.c_str()));
  auto manager = std::make_shared<HeartbeatManager>(
      stateDir, makeTestEdenFsEventsLogger());
  folly::ManualExecutor executor;

  EXPECT_TRUE(manager->checkForPreviousHeartbeat(
      false, std::nullopt, false, &executor));
  executor.drain();

  EXPECT_FALSE(std::filesystem::exists(leftover.string()));
}
