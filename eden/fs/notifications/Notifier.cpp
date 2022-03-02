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

namespace facebook {
namespace eden {

bool Notifier::canShowNotification() {
  auto now = std::chrono::steady_clock::now();
  auto last = lastShown_.wlock();
  // TODO(@cuev) add logic to disable notifications altogether
  if (*last) {
    auto expiry = last->value() +
        config_.getEdenConfig()->notificationInterval.getValue();
    if (now < expiry) {
      return false;
    }
  }
  return true;
}

} // namespace eden
} // namespace facebook
