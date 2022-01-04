/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <memory>

#include <folly/Synchronized.h>

#include "eden/fs/config/gen-cpp2/eden_config_types.h"

namespace facebook::eden {

class EdenConfig;

/**
 * An interface that defines how to obtain a possibly reloaded EdenConfig
 * instance.
 */
class ReloadableConfig {
 public:
  explicit ReloadableConfig(std::shared_ptr<const EdenConfig> config);
  ReloadableConfig(
      std::shared_ptr<const EdenConfig> config,
      ConfigReloadBehavior reload);
  ~ReloadableConfig();

  /**
   * Get the EdenConfig data.
   *
   * The config data may be reloaded from disk depending on the value of the
   * reload parameter.
   */
  std::shared_ptr<const EdenConfig> getEdenConfig(
      ConfigReloadBehavior reload = ConfigReloadBehavior::AutoReload);

 private:
  struct ConfigState {
    explicit ConfigState(const std::shared_ptr<const EdenConfig>& config)
        : config{config} {}
    std::shared_ptr<const EdenConfig> config;
  };

  folly::Synchronized<ConfigState> state_;
  std::atomic<std::chrono::steady_clock::time_point::rep> lastCheck_{};

  // Reload behavior, when set this overrides reload behavior passed to methods
  // This is used in tests where we want to set the manually set the EdenConfig
  // and avoid reloading it from disk.
  std::optional<ConfigReloadBehavior> reloadBehavior_{std::nullopt};
};

} // namespace facebook::eden
