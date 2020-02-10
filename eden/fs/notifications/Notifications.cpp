/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/notifications/Notifications.h"

#include <folly/Subprocess.h>
#include <folly/futures/Future.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/utils/SystemError.h"

namespace facebook {
namespace eden {

bool Notifications::canShowNotification() {
  auto now = std::chrono::steady_clock::now();
  auto last = lastShown_.wlock();
  if (*last) {
    auto expiry = last->value() +
        config_.getEdenConfig()->notificationInterval.getValue();
    if (now < expiry) {
      return false;
    }
  }
  return true;
}

namespace {
bool isGenericConnectivityError(const std::exception& err) {
  int errnum = EIO;
  if (auto* sys = dynamic_cast<const std::system_error*>(&err)) {
    if (isErrnoError(*sys)) {
      errnum = sys->code().value();
    }
  } else if (auto* timeout = dynamic_cast<const folly::FutureTimeout*>(&err)) {
    errnum = ETIMEDOUT;
  }
  return errnum == EIO || errnum == ETIMEDOUT;
}
} // namespace

void Notifications::showGenericErrorNotification(const std::exception& err) {
  if (!isGenericConnectivityError(err)) {
    return;
  }

  if (!canShowNotification()) {
    return;
  }
  *lastShown_.wlock() = std::chrono::steady_clock::now();

  folly::Subprocess proc(
      {"/bin/sh",
       "-c",
       config_.getEdenConfig()->genericErrorNotificationCommand.getValue()},
      folly::Subprocess::Options().detach());
}
} // namespace eden
} // namespace facebook
