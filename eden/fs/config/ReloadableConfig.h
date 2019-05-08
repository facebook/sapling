/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <chrono>
#include <memory>

#include <folly/Synchronized.h>

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
   * Get the EdenConfig; We check for changes in the config files, reload as
   * necessary and return an updated EdenConfig. The update checks are
   * throttleSeconds to kEdenConfigMinPollSeconds. If 'skipUpdate' is set, no
   * update check is performed and the current EdenConfig is returned.
   */
  std::shared_ptr<const EdenConfig> getEdenConfig(bool skipUpdate = false);

 private:
  struct ConfigState {
    explicit ConfigState(const std::shared_ptr<const EdenConfig>& config)
        : config{config} {}
    std::chrono::steady_clock::time_point lastCheck;
    std::shared_ptr<const EdenConfig> config;
  };

  /**
   * Check if any if system or user configuration files have changed. If so,
   * parse and apply the changes to the EdenConfig. This method throttles
   * update requests to once per kEdenConfigMinPollSeconds.
   * @return the updated EdenConfig.
   */
  std::shared_ptr<const EdenConfig> getUpdatedEdenConfig();

  folly::Synchronized<ConfigState> state_;
};

} // namespace eden
} // namespace facebook
