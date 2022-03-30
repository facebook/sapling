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

/**
 * No-op notifier, used when notifications are globally disabled.
 */
class NullNotifier : public Notifier {
 public:
  explicit NullNotifier(std::shared_ptr<ReloadableConfig> edenConfig)
      : Notifier(std::move(edenConfig)) {}

  virtual void showNetworkNotification(const std::exception& /*err*/) override {
  }
};

} // namespace facebook::eden
