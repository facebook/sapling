/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/privhelper/PrivHelper.h"

#include <folly/File.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>

namespace facebook::eden {

void PrivHelper::setLogFileBlocking(folly::File logFile) {
  folly::EventBase evb;
  attachEventBase(&evb);

  auto future = setLogFile(std::move(logFile));
  if (future.isReady()) {
    std::move(future).get();
    return;
  }

  future = std::move(future).ensure([&evb] { evb.terminateLoopSoon(); });
  evb.loopForever();
  std::move(future).get();
}

void PrivHelper::setDaemonTimeoutBlocking(std::chrono::nanoseconds duration) {
  folly::EventBase evb;
  attachEventBase(&evb);

  auto future = setDaemonTimeout(std::move(duration));
  if (future.isReady()) {
    std::move(future).get();
    return;
  }

  future = std::move(future).ensure([&evb] { evb.terminateLoopSoon(); });
  evb.loopForever();
  std::move(future).get();
}

void PrivHelper::setMemoryPriorityForProcessBlocking(
    pid_t pid,
    int targetPriority) {
  folly::EventBase evb;
  attachEventBase(&evb);

  auto future = setMemoryPriorityForProcess(pid, targetPriority);
  if (future.isReady()) {
    std::move(future).get();
    return;
  }

  future = std::move(future).ensure([&evb] { evb.terminateLoopSoon(); });
  evb.loopForever();
  std::move(future).get();
}

} // namespace facebook::eden
