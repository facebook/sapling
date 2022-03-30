/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/notifications/Notifier.h"

namespace facebook {
namespace eden {

class ReloadableConfig;

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

  virtual void showNetworkNotification(const std::exception& err) override;
};

} // namespace eden
} // namespace facebook
