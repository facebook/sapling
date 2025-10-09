/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/concurrency/memory/AtomicReadMostlyMainPtr.h>

namespace facebook::eden {

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
   * This merely returns the in-memory copy of the config, it does not attempt
   * to reload it. Clients desiring the config to be reloaded must call
   * `maybeReload` periodically.
   * Note: `maybeReload` is currently scheduled to run every 5 minutes as
   * the periodic task of EdenServer::reloadConfigTask_.
   */
  folly::ReadMostlySharedPtr<const EdenConfig> getEdenConfig() const {
    return state_.load();
  }

  /**
   * If the on-disk config has changed, reload it.
   *
   * Subsequent calls to getEdenConfig() will return the new updated config.
   */
  void maybeReload();

 private:
  folly::AtomicReadMostlyMainPtr<const EdenConfig> state_;
};

} // namespace facebook::eden
