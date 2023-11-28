/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/ServerState.h"

#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/nfs/NfsServer.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/FsEventLogger.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

DEFINE_bool(
    fault_injection_block_mounts,
    false,
    "Block mount attempts via the fault injection framework.  "
    "Requires --enable_fault_injection.");

DEFINE_bool(
    fault_injection_fail_opening_local_store,
    false,
    "Causes the local store to fail to open on startup. "
    "Requires --enable_fault_injection.");

namespace facebook::eden {
/** Throttle Ignore change checks, max of 1 per kUserIgnoreMinPollSeconds */
constexpr std::chrono::seconds kUserIgnoreMinPollSeconds{5};

/** Throttle Ignore change checks, max of 1 per kSystemIgnoreMinPollSeconds */
constexpr std::chrono::seconds kSystemIgnoreMinPollSeconds{5};

ServerState::ServerState(
    UserInfo userInfo,
    EdenStatsPtr edenStats,
    std::shared_ptr<PrivHelper> privHelper,
    std::shared_ptr<UnboundedQueueExecutor> threadPool,
    std::shared_ptr<folly::Executor> fsChannelThreadPool,
    std::shared_ptr<Clock> clock,
    std::shared_ptr<ProcessInfoCache> processInfoCache,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::shared_ptr<IHiveLogger> hiveLogger,
    std::shared_ptr<ReloadableConfig> reloadableConfig,
    const EdenConfig& initialConfig,
    [[maybe_unused]] folly::EventBase* mainEventBase,
    std::shared_ptr<Notifier> notifier,
    bool enableFaultDetection)
    : userInfo_{std::move(userInfo)},
      edenStats_{std::move(edenStats)},
      privHelper_{std::move(privHelper)},
      threadPool_{std::move(threadPool)},
      fsChannelThreadPool_{std::move(fsChannelThreadPool)},
      clock_{std::move(clock)},
      processInfoCache_{std::move(processInfoCache)},
      structuredLogger_{std::move(structuredLogger)},
      hiveLogger_{std::move(hiveLogger)},
      faultInjector_{std::make_unique<FaultInjector>(enableFaultDetection)},
      nfs_{
          initialConfig.enableNfsServer.getValue()
              ? std::make_shared<NfsServer>(
                    privHelper_.get(),
                    mainEventBase,
                    fsChannelThreadPool_,
                    initialConfig.runInternalRpcbind.getValue(),
                    structuredLogger_)
              : nullptr},
      config_{std::move(reloadableConfig)},
      userIgnoreFileMonitor_{CachedParsedFileMonitor<GitIgnoreFileParser>{
          initialConfig.userIgnoreFile.getValue(),
          kUserIgnoreMinPollSeconds}},
      systemIgnoreFileMonitor_{CachedParsedFileMonitor<GitIgnoreFileParser>{
          initialConfig.systemIgnoreFile.getValue(),
          kSystemIgnoreMinPollSeconds}},
      notifier_{std::move(notifier)},
      fsEventLogger_{
          initialConfig.requestSamplesPerMinute.getValue()
              ? std::make_shared<FsEventLogger>(config_, hiveLogger_)
              : nullptr} {
  // It would be nice if we eventually built a more generic mechanism for
  // defining faults to be configured on start up.  (e.g., loading this from
  // the EdenConfig).
  //
  // For now, blocking mounts and failing localstore opening are the main
  // things we want to be able to control on startup (since mounting and
  // opening localstore occurs automatically during startup).  Add a two-off
  // command
  // line flag to control this for now, until we build a more generic
  // mechanism.
  if (FLAGS_fault_injection_block_mounts) {
    faultInjector_->injectBlock("mount", ".*");
  }
  if (FLAGS_fault_injection_fail_opening_local_store) {
    faultInjector_->injectError(
        "open_local_store",
        ".*",
        std::runtime_error{"Some random error opening LocalStore"});
  }
}

ServerState::~ServerState() = default;

std::shared_ptr<const EdenConfig> ServerState::getEdenConfig(
    ConfigReloadBehavior reload) {
  return config_->getEdenConfig(reload);
}

std::unique_ptr<TopLevelIgnores> ServerState::getTopLevelIgnores() {
  // Update EdenConfig to detect changes to the system or user ignore files
  auto edenConfig = getEdenConfig();

  // Get the potentially changed system/user ignore files
  auto userIgnoreFile = edenConfig->userIgnoreFile.getValue();
  auto systemIgnoreFile = edenConfig->systemIgnoreFile.getValue();

  // Get the userIgnoreFile
  GitIgnore userGitIgnore{};
  auto fcResult =
      userIgnoreFileMonitor_.wlock()->getFileContents(userIgnoreFile);
  if (fcResult.hasValue()) {
    userGitIgnore = fcResult.value();
  }

  // Get the systemIgnoreFile
  GitIgnore systemGitIgnore{};
  fcResult =
      systemIgnoreFileMonitor_.wlock()->getFileContents(systemIgnoreFile);
  if (fcResult.hasValue()) {
    systemGitIgnore = fcResult.value();
  }
  return std::make_unique<TopLevelIgnores>(
      std::move(userGitIgnore), std::move(systemGitIgnore));
}

} // namespace facebook::eden
