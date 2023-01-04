/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <atomic>
#include <chrono>
#include <memory>

#include <folly/Synchronized.h>

#include "eden/fs/config/gen-cpp2/eden_config_types.h"

namespace facebook::eden {

class EdenConfig;

// It's unclear to me (chadaustin) whether `atomic<time_point>` is
// defined and lock-free. It seems to work on recent compilers and C++
// versions we support. If necessary, we can replace this typedef with
// an AtomicTimePoint class internally represented as
// `atomic<duration>`.

template <typename Clock>
using AtomicTimePoint = std::atomic<typename Clock::time_point>;

// This static_assert is not strictly necessary. But if we ever build
// EdenFS on a compiler that wraps `atomic<time_point>` in a mutex, we
// probably want to do something about it.
static_assert(
    AtomicTimePoint<std::chrono::steady_clock>::is_always_lock_free,
    "atomic<time_point> is not lock-free - consider writing AtomicTimePoint");

/**
 * An interface that defines how to obtain a possibly reloaded EdenConfig
 * instance.
 */
class ReloadableConfig {
 public:
  explicit ReloadableConfig(std::shared_ptr<const EdenConfig> config);

  /**
   * Create a ReloadableConfig with a hardcoded, overridden reload behavior. The
   * reload behavior passed to `getEdenConfig` will be ignored.
   */
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
    std::shared_ptr<const EdenConfig> config;
  };

  folly::Synchronized<ConfigState> state_;
  AtomicTimePoint<std::chrono::steady_clock> lastCheck_{
      std::chrono::steady_clock::time_point{},
  };

  // When set, this overrides reload behavior passed to `getEdenConfig`.
  // Used in tests where we want to set the manually set the EdenConfig and
  // avoid reloading it from disk.
  std::optional<ConfigReloadBehavior> reloadBehavior_;
};

} // namespace facebook::eden
