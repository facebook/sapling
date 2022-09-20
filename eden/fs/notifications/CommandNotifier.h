/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/notifications/Notifier.h"

namespace facebook::eden {

class ReloadableConfig;
struct InodePopulationReport;

/**
 * Show a generic "something went wrong" notification to the interactive
 * user.
 *
 * This is implemented by invoking the command specified by the
 * configuration value named:
 * notifications:generic-connectivity-notification-cmd
 */
class CommandNotifier : public Notifier {
 public:
  explicit CommandNotifier(std::shared_ptr<ReloadableConfig> edenConfig)
      : Notifier(std::move(edenConfig)) {}

  void showNotification(
      std::string_view notifTitle,
      std::string_view notifBody,
      std::string_view mount) override;

  void showNetworkNotification(const std::exception& err) override;

  void signalCheckout(size_t /*numActive*/) override {}

  void registerInodePopulationReportCallback(
      std::function<std::vector<InodePopulationReport>()> /*callback*/)
      override {}
};

} // namespace facebook::eden
