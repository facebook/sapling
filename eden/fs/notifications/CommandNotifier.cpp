/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/notifications/CommandNotifier.h"

#include <folly/futures/Future.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/utils/SpawnedProcess.h"
#include "eden/fs/utils/SystemError.h"

namespace facebook::eden {

namespace {
bool isGenericConnectivityError(const std::exception& err) {
  int errnum = EIO;
  if (auto* sys = dynamic_cast<const std::system_error*>(&err)) {
    if (isErrnoError(*sys)) {
      errnum = sys->code().value();
    }
  } else if (dynamic_cast<const folly::FutureTimeout*>(&err)) {
    errnum = ETIMEDOUT;
  }
  return errnum == EIO || errnum == ETIMEDOUT;
}
} // namespace

void CommandNotifier::showNotification(
    std::string_view notifTitle,
    std::string_view notifBody,
    std::string_view mount) {
  XLOG(
      WARN,
      "showNotification is unimplemented for CommandNotifiers: {}: {}: {}",
      mount,
      notifTitle,
      notifBody);
}

void CommandNotifier::showNetworkNotification(const std::exception& err) {
  if (!isGenericConnectivityError(err)) {
    return;
  }

  if (!updateLastShown()) {
    return;
  }

  std::vector<std::string> args;
  if (folly::kIsWindows) {
    args.emplace_back("powershell");
    args.emplace_back("-NoProfile");
    args.emplace_back("-Command");
  } else {
    args.emplace_back("/bin/sh");
    args.emplace_back("-c");
  }

  args.emplace_back(
      config_->getEdenConfig()->genericErrorNotificationCommand.getValue());

  SpawnedProcess(args).detach();
}

} // namespace facebook::eden
