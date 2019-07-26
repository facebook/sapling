/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <sys/types.h>
#include <unistd.h>
#include <utility>

#include "eden/fs/utils/ProcessAccessLog.h"
#include "eden/fs/utils/ProcessNameCache.h"

using ::testing::Contains;
using ::testing::ElementsAre;
using ::testing::Eq;
using ::testing::Key;
using namespace facebook::eden;
using namespace std::chrono_literals;

TEST(ProcessAccessLog, emptyLogHasNoAccesses) {
  auto log = ProcessAccessLog{std::make_shared<ProcessNameCache>()};
  EXPECT_THAT(log.getAccessCounts(1s), ElementsAre());
}

TEST(ProcessAccessLog, accessAddsProcessToProcessNameCache) {
  auto pid = ::getpid();
  auto processNameCache = std::make_shared<ProcessNameCache>();
  auto log = ProcessAccessLog{processNameCache};
  log.recordAccess(::getpid(), ProcessAccessLog::OTHER);
  EXPECT_THAT(processNameCache->getAllProcessNames(), Contains(Key(Eq(pid))));
}

TEST(ProcessAccessLog, accessIncrementsAccessCount) {
  auto pid = pid_t{42};
  auto log = ProcessAccessLog{std::make_shared<ProcessNameCache>()};

  log.recordAccess(pid, ProcessAccessLog::READ);
  log.recordAccess(pid, ProcessAccessLog::WRITE);
  log.recordAccess(pid, ProcessAccessLog::OTHER);

  auto ac = AccessCounts{};
  ac.total = 3;
  ac.reads = 1;
  ac.writes = 1;

  EXPECT_THAT(log.getAccessCounts(1s), Contains(std::pair{pid, ac}));
}
