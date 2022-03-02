/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <optional>
#include "eden/fs/config/ReloadableConfig.h"

namespace facebook {
namespace eden {

class ReloadableConfig;

/* The intent is that that class will allow us to show a desktop "toast"
 * notification to the user, but in some environments it is possible that it
 * might instead trigger eg: a Workplace Messenger chat notification.
 *
 * This Notifications instance will throttle the rate at which these
 * occur based on the value of the notifications:interval configuration
 * which defaults to a reasonable value to avoid spamming the user.
 *
 * Users can also disable notifications altogether.
 */
class Notifier {
 public:
  explicit Notifier(ReloadableConfig& edenConfig) : config_(edenConfig) {}
  virtual ~Notifier() {}

  /**
   * Show a network notification to the interactive user.
   */
  virtual void showNetworkNotification(const std::exception& err) = 0;

 protected:
  bool canShowNotification();
  ReloadableConfig& config_;
  folly::Synchronized<std::optional<std::chrono::steady_clock::time_point>>
      lastShown_;
};

} // namespace eden
} // namespace facebook
