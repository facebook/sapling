/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <memory>

#include <folly/Synchronized.h>

#include "eden/fs/config/gen-cpp2/eden_config_types.h"

namespace facebook {
namespace eden {

class EdenConfig;

/**
 * An interface that defines how to obtain a possibly reloaded EdenConfig
 * instance.
 */
class ReloadableConfig {
 public:
  explicit ReloadableConfig(std::shared_ptr<const EdenConfig> config);
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
  std::atomic<std::chrono::steady_clock::time_point::rep> lastCheck_;
};

} // namespace eden
} // namespace facebook
