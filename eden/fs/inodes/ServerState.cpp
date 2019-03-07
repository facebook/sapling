/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/ServerState.h"

#include <folly/logging/xlog.h>

#include "eden/fs/config/EdenConfig.h"
#ifndef EDEN_WIN
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#endif
#include "eden/fs/inodes/TopLevelIgnores.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

DEFINE_bool(
    enable_fault_injection,
    false,
    "Enable the fault injection framework.");
DEFINE_bool(
    fault_injection_block_mounts,
    false,
    "Block mount attempts via the fault injection framework.  "
    "Requires --enable_fault_injection.");

namespace facebook {
namespace eden {

/** Throttle EdenConfig change checks, max of 1 per kEdenConfigMinPollSeconds */
constexpr std::chrono::seconds kEdenConfigMinPollSeconds{5};

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
    std::shared_ptr<const EdenConfig> edenConfig)
    : userInfo_{std::move(userInfo)},
      privHelper_{std::move(privHelper)},
      threadPool_{std::move(threadPool)},
      clock_{std::move(clock)},
      processNameCache_{std::move(processNameCache)},
      faultInjector_{new FaultInjector(FLAGS_enable_fault_injection)},
      configState_{ConfigState{edenConfig}},
      userIgnoreFileMonitor_{CachedParsedFileMonitor<GitIgnoreFileParser>{
          edenConfig->getUserIgnoreFile(),
          kUserIgnoreMinPollSeconds}},
      systemIgnoreFileMonitor_{CachedParsedFileMonitor<GitIgnoreFileParser>{
          edenConfig->getSystemIgnoreFile(),
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

std::shared_ptr<const EdenConfig> ServerState::getEdenConfig(bool skipUpdate) {
  if (!skipUpdate) {
    return getUpdatedEdenConfig();
  }
  return configState_.rlock()->config;
}

// TODO: Update this monitoring code to use FileChangeMonitor.
std::shared_ptr<const EdenConfig> ServerState::getUpdatedEdenConfig() {
  std::chrono::steady_clock::time_point now = std::chrono::steady_clock::now();
  // Throttle the updates
  auto cfgStatePtr = configState_.wlock();
  if ((now - cfgStatePtr->lastCheck) > kEdenConfigMinPollSeconds) {
    // Update the throttle setting - to prevent thrashing.
    cfgStatePtr->lastCheck = now;
    bool userConfigChanged = cfgStatePtr->config->hasUserConfigFileChanged();
    bool systemConfigChanged =
        cfgStatePtr->config->hasSystemConfigFileChanged();
    if (userConfigChanged || systemConfigChanged) {
      auto newConfig = std::make_shared<EdenConfig>(*cfgStatePtr->config);
      if (userConfigChanged) {
        newConfig->loadUserConfig();
      }
      if (systemConfigChanged) {
        newConfig->loadSystemConfig();
      }
      cfgStatePtr->config = std::move(newConfig);
    }
  }
  return cfgStatePtr->config;
}

std::unique_ptr<TopLevelIgnores> ServerState::getTopLevelIgnores() {
  // Update EdenConfig to detect changes to the system or user ignore files
  auto edenConfig = getEdenConfig();

  // Get the potentially changed system/user ignore files
  auto userIgnoreFile = edenConfig->getUserIgnoreFile();
  auto systemIgnoreFile = edenConfig->getSystemIgnoreFile();

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
