/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/ServerState.h"

#include <folly/logging/xlog.h>
#include <gflags/gflags.h>

#include "eden/fs/config/EdenConfig.h"
#ifndef _WIN32
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#endif
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

DEFINE_bool(
    fault_injection_block_mounts,
    false,
    "Block mount attempts via the fault injection framework.  "
    "Requires --enable_fault_injection.");

namespace facebook {
namespace eden {

/** Throttle Ignore change checks, max of 1 per kUserIgnoreMinPollSeconds */
constexpr std::chrono::seconds kUserIgnoreMinPollSeconds{5};

/** Throttle Ignore change checks, max of 1 per kSystemIgnoreMinPollSeconds */
constexpr std::chrono::seconds kSystemIgnoreMinPollSeconds{5};

ServerState::ServerState(
    UserInfo userInfo,
    std::shared_ptr<PrivHelper> privHelper,
    std::shared_ptr<UnboundedQueueExecutor> threadPool,
    std::shared_ptr<Clock> clock,
    std::shared_ptr<ProcessNameCache> processNameCache,
    std::shared_ptr<const EdenConfig> edenConfig,
    bool enableFaultDetection)
    : userInfo_{std::move(userInfo)},
      privHelper_{std::move(privHelper)},
      threadPool_{std::move(threadPool)},
      clock_{std::move(clock)},
      processNameCache_{std::move(processNameCache)},
      faultInjector_{std::make_unique<FaultInjector>(enableFaultDetection)},
      config_{edenConfig},
      userIgnoreFileMonitor_{CachedParsedFileMonitor<GitIgnoreFileParser>{
          edenConfig->userIgnoreFile.getValue(),
          kUserIgnoreMinPollSeconds}},
      systemIgnoreFileMonitor_{CachedParsedFileMonitor<GitIgnoreFileParser>{
          edenConfig->systemIgnoreFile.getValue(),
          kSystemIgnoreMinPollSeconds}} {
  // It would be nice if we eventually built a more generic mechanism for
  // defining faults to be configured on start up.  (e.g., loading this from the
  // EdenConfig).
  //
  // For now, blocking mounts is the main thing we want to be able to control on
  // startup (since mounting occurs automatically during startup).  Add a
  // one-off command line flag to control this for now, until we build a more
  // generic mechanism.
  if (FLAGS_fault_injection_block_mounts) {
    faultInjector_->injectBlock("mount", ".*");
  }
}

ServerState::~ServerState() {}

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

} // namespace eden
} // namespace facebook
