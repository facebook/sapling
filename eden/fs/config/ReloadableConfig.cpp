/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
    ConfigReloadBehavior reload) {
  if (reload == ConfigReloadBehavior::NoReload) {
    return state_.rlock()->config;
  }

  // TODO: Update this monitoring code to use FileChangeMonitor.
  std::chrono::steady_clock::time_point now = std::chrono::steady_clock::now();
  auto state = state_.wlock();

  // Throttle the updates when using ConfigReloadBehavior::AutoReload
  if (reload == ConfigReloadBehavior::AutoReload &&
      (now - state->lastCheck) < kEdenConfigMinPollSeconds) {
    return state->config;
  }
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
