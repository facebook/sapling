/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/notifications/Notifier.h"

#include <folly/futures/Future.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/utils/SystemError.h"

namespace facebook::eden {

bool Notifier::updateLastShown() {
  auto now = std::chrono::steady_clock::now();
  auto last = lastShown_.wlock();
  if (config_->getEdenConfig()->enableNotifications.getValue() && *last) {
    auto expiry = last->value() +
        config_->getEdenConfig()->notificationInterval.getValue();
    if (now < expiry) {
      return false;
    }
  }
  *last = std::chrono::steady_clock::now();
  return true;
}

} // namespace facebook::eden
