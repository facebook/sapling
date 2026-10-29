/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <thread>
#include <utility>

#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/fs/utils/ProcessAccessLog.h"

using ::testing::Contains;
using ::testing::ElementsAre;
using ::testing::Eq;
using ::testing::Key;
using ::testing::UnorderedElementsAre;
using namespace facebook::eden;
using namespace std::chrono_literals;

TEST(ProcessAccessLog, emptyLogHasNoAccesses) {
  auto log = ProcessAccessLog{std::make_shared<ProcessInfoCache>()};
  EXPECT_THAT(log.getAccessCounts(10s), ElementsAre());
}

TEST(ProcessAccessLog, accessIncrementsAccessCount) {
  auto pid = pid_t{42};
  auto log = ProcessAccessLog{std::make_shared<ProcessInfoCache>()};

  log.recordAccess(pid, ProcessAccessLog::AccessType::FsChannelRead);
  log.recordAccess(pid, ProcessAccessLog::AccessType::FsChannelWrite);
  log.recordAccess(pid, ProcessAccessLog::AccessType::FsChannelOther);
  log.recordAccess(
      pid, ProcessAccessLog::AccessType::FsChannelBackingStoreImport);

  auto ac = AccessCounts{};
  ac.fsChannelTotal() = 3;
  ac.fsChannelReads() = 1;
  ac.fsChannelWrites() = 1;
  ac.fsChannelBackingStoreImports() = 1;

  EXPECT_THAT(log.getAccessCounts(10s), Contains(std::pair{pid, ac}));
}

TEST(ProcessAccessLog, accessAddsProcessToProcessInfoCache) {
  auto pid = pid_t{1};
  auto processInfoCache = std::make_shared<ProcessInfoCache>();
  auto log = ProcessAccessLog{processInfoCache};
  log.recordAccess(pid, ProcessAccessLog::AccessType::FsChannelOther);
  EXPECT_THAT(processInfoCache->getAllProcessInfos(), Contains(Key(Eq(pid))));
}

// Pins the current behavior: the first log a thread records into owns that
// thread's bucket for good, so `second` never sees pids 2 and 3 and the
// counts land in `first` instead. The fix flips these expectations.
TEST(ProcessAccessLog, oneThreadRecordingIntoTwoLogs) {
  auto processInfoCache = std::make_shared<ProcessInfoCache>();
  auto first = std::make_unique<ProcessAccessLog>(processInfoCache);
  auto second = std::make_unique<ProcessAccessLog>(processInfoCache);

  std::thread worker([&] {
    first->recordAccess(1, ProcessAccessLog::AccessType::FsChannelRead);
    second->recordAccess(2, ProcessAccessLog::AccessType::FsChannelRead);
    EXPECT_THAT(
        first->getAccessCounts(10s), UnorderedElementsAre(Key(1), Key(2)));
    EXPECT_THAT(second->getAccessCounts(10s), ElementsAre());

    first.reset();
    second->recordAccess(3, ProcessAccessLog::AccessType::FsChannelRead);
    EXPECT_THAT(second->getAccessCounts(10s), ElementsAre());
  });
  worker.join();
}
