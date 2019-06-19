/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/ProcessAccessLog.h"
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <sys/types.h>
#include <unistd.h>
#include <utility>
#include "eden/fs/utils/ProcessNameCache.h"

using ::testing::Contains;
using ::testing::ElementsAre;
using ::testing::Eq;
using ::testing::Key;
using namespace facebook::eden;
using namespace std::chrono_literals;

TEST(ProcessAccessLog, emptyLogHasNoAccesses) {
  auto log = ProcessAccessLog{std::make_shared<ProcessNameCache>()};
  EXPECT_THAT(log.getAllAccesses(1s), ElementsAre());
}

TEST(ProcessAccessLog, accessAddsProcessToProcessNameCache) {
  auto pid = ::getpid();
  auto processNameCache = std::make_shared<ProcessNameCache>();
  auto log = ProcessAccessLog{processNameCache};
  log.recordAccess(::getpid());
  EXPECT_THAT(processNameCache->getAllProcessNames(), Contains(Key(Eq(pid))));
}

TEST(ProcessAccessLog, accessIncrementsAccessCount) {
  auto pid = pid_t{42};
  auto log = ProcessAccessLog{std::make_shared<ProcessNameCache>()};
  log.recordAccess(pid);
  log.recordAccess(pid);
  log.recordAccess(pid);
  EXPECT_THAT(log.getAllAccesses(1s), Contains(std::pair{pid, 3}));
}
