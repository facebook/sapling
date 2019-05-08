/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/config/ReloadableConfig.h"

#include "eden/fs/config/EdenConfig.h"

namespace {
/** Throttle EdenConfig change checks, max of 1 per kEdenConfigMinPollSeconds */
constexpr std::chrono::seconds kEdenConfigMinPollSeconds{5};
} // namespace

namespace facebook {
namespace eden {

ReloadableConfig::ReloadableConfig(std::shared_ptr<const EdenConfig> config)
    : state_{ConfigState{config}} {}

ReloadableConfig::~ReloadableConfig() {}

std::shared_ptr<const EdenConfig> ReloadableConfig::getEdenConfig(
    bool skipUpdate) {
  if (!skipUpdate) {
    return getUpdatedEdenConfig();
  }
  return state_.rlock()->config;
}

// TODO: Update this monitoring code to use FileChangeMonitor.
std::shared_ptr<const EdenConfig> ReloadableConfig::getUpdatedEdenConfig() {
  std::chrono::steady_clock::time_point now = std::chrono::steady_clock::now();
  // Throttle the updates
  auto state = state_.wlock();
  if ((now - state->lastCheck) < kEdenConfigMinPollSeconds) {
    return state->config;
  }

  // Update the throttle setting - to prevent thrashing.
  state->lastCheck = now;
  bool userConfigChanged = state->config->hasUserConfigFileChanged();
  bool systemConfigChanged = state->config->hasSystemConfigFileChanged();
  if (userConfigChanged || systemConfigChanged) {
    auto newConfig = std::make_shared<EdenConfig>(*state->config);
    if (userConfigChanged) {
      newConfig->loadUserConfig();
    }
    if (systemConfigChanged) {
      newConfig->loadSystemConfig();
    }
    state->config = std::move(newConfig);
  }
  return state->config;
}

} // namespace eden
} // namespace facebook
