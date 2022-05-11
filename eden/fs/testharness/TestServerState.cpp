/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/TestServerState.h"

#include <folly/executors/ManualExecutor.h>
#include <memory>

#include "eden/common/utils/ProcessNameCache.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/notifications/CommandNotifier.h"
#include "eden/fs/telemetry/IHiveLogger.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeClock.h"
#include "eden/fs/testharness/FakePrivHelper.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"
#include "eden/fs/utils/UserInfo.h"

namespace facebook::eden {

std::shared_ptr<ServerState> createTestServerState() {
  auto executor = std::make_shared<folly::ManualExecutor>();
  auto edenConfig = EdenConfig::createTestEdenConfig();
  auto reloadableConfig = std::make_shared<ReloadableConfig>(edenConfig);

  return std::make_shared<ServerState>(
      UserInfo::lookup(),
      std::make_shared<FakePrivHelper>(),
      std::make_shared<UnboundedQueueExecutor>(executor),
      std::make_shared<FakeClock>(),
      std::make_shared<ProcessNameCache>(),
      std::make_shared<NullStructuredLogger>(),
      std::make_shared<NullHiveLogger>(),
      std::make_shared<ReloadableConfig>(edenConfig),
      *edenConfig,
      nullptr,
      std::make_shared<CommandNotifier>(reloadableConfig),
      /*enableFaultInjection=*/true);
};

} // namespace facebook::eden
