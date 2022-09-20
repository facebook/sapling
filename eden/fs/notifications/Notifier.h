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

namespace facebook::eden {

/* A user issued command (currently only in windows) generates a per mount
 * vector of InodePopulationReports. */
struct InodePopulationReport {
  std::string mountName;
  size_t inodeCount;
};

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
  explicit Notifier(std::shared_ptr<ReloadableConfig> edenConfig)
      : config_(std::move(edenConfig)) {}
  virtual ~Notifier() {}

  /**
   * Show a custom notification to the interactive user.
   */
  virtual void showNotification(
      std::string_view notifTitle,
      std::string_view notifBody,
      std::string_view mount) = 0;

  /**
   * Show a network error notification to the interactive user.
   */
  virtual void showNetworkNotification(const std::exception& err) = 0;

  /**
   * Signal to the notifier that the count of live checkout operations has
   * changed.
   */
  virtual void signalCheckout(size_t numActive) = 0;

  /**
   * Register InodePopulationReport callback with the notifier.
   */
  virtual void registerInodePopulationReportCallback(
      std::function<std::vector<InodePopulationReport>()> callback) = 0;

 protected:
  bool updateLastShown();
  std::shared_ptr<ReloadableConfig> config_;
  folly::Synchronized<std::optional<std::chrono::steady_clock::time_point>>
      lastShown_;
};

} // namespace facebook::eden
