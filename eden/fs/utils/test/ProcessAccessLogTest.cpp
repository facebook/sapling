/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <sys/types.h>
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
  EXPECT_THAT(log.getAccessCounts(10s), ElementsAre());
}

TEST(ProcessAccessLog, accessIncrementsAccessCount) {
  auto pid = pid_t{42};
  auto log = ProcessAccessLog{std::make_shared<ProcessNameCache>()};

  log.recordAccess(pid, ProcessAccessLog::AccessType::FsChannelRead);
  log.recordAccess(pid, ProcessAccessLog::AccessType::FsChannelWrite);
  log.recordAccess(pid, ProcessAccessLog::AccessType::FsChannelOther);
  log.recordAccess(
      pid, ProcessAccessLog::AccessType::FsChannelBackingStoreImport);

  auto ac = AccessCounts{};
  ac.fsChannelTotal_ref() = 3;
  ac.fsChannelReads_ref() = 1;
  ac.fsChannelWrites_ref() = 1;
  ac.fsChannelBackingStoreImports_ref() = 1;

  EXPECT_THAT(log.getAccessCounts(10s), Contains(std::pair{pid, ac}));
}

TEST(ProcessAccessLog, accessAddsProcessToProcessNameCache) {
  auto pid = pid_t{1};
  auto processNameCache = std::make_shared<ProcessNameCache>();
  auto log = ProcessAccessLog{processNameCache};
  log.recordAccess(pid, ProcessAccessLog::AccessType::FsChannelOther);
  EXPECT_THAT(processNameCache->getAllProcessNames(), Contains(Key(Eq(pid))));
}
