/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/ReloadableConfig.h"

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/utils/Bug.h"

namespace {
/// Throttle change checks to a maximum of one per
/// kEdenConfigMinimumPollDuration.
constexpr std::chrono::seconds kEdenConfigMinimumPollDuration{5};
} // namespace

namespace facebook {
namespace eden {

ReloadableConfig::ReloadableConfig(std::shared_ptr<const EdenConfig> config)
    : state_{ConfigState{config}} {}

ReloadableConfig::~ReloadableConfig() {}

std::shared_ptr<const EdenConfig> ReloadableConfig::getEdenConfig(
    ConfigReloadBehavior reload) {
  auto now = std::chrono::steady_clock::now();

  // TODO: Update this monitoring code to use FileChangeMonitor.
  bool shouldReload;
  switch (reload) {
    case ConfigReloadBehavior::NoReload:
      shouldReload = false;
      break;
    case ConfigReloadBehavior::ForceReload:
      shouldReload = true;
      break;
    case ConfigReloadBehavior::AutoReload: {
      auto lastCheck = std::chrono::steady_clock::time_point{
          std::chrono::steady_clock::duration{
              lastCheck_.load(std::memory_order_acquire)}};
      shouldReload = now - lastCheck >= kEdenConfigMinimumPollDuration;
      break;
    }
    default:
      EDEN_BUG() << "Unexpected reload flag: " << static_cast<int>(reload);
  }

  if (!shouldReload) {
    return state_.rlock()->config;
  }

  auto state = state_.wlock();

  // Throttle the updates when using ConfigReloadBehavior::AutoReload
  lastCheck_.store(now.time_since_epoch().count(), std::memory_order_release);

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
