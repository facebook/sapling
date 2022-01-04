/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/privhelper/PrivHelper.h"

#include <folly/File.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>

namespace facebook::eden {

#ifndef _WIN32

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

void PrivHelper::setUseEdenFsBlocking(bool useEdenFs) {
  folly::EventBase evb;
  attachEventBase(&evb);

  auto future = setUseEdenFs(useEdenFs);
  if (future.isReady()) {
    std::move(future).get();
    return;
  }

  future = std::move(future).ensure([&evb] { evb.terminateLoopSoon(); });
  evb.loopForever();
  std::move(future).get();
}

#else // _WIN32

void PrivHelper::setLogFileBlocking(folly::File) {}

void PrivHelper::setDaemonTimeoutBlocking(std::chrono::nanoseconds) {}
void PrivHelper::setUseEdenFsBlocking(bool) {}

#endif // _WIN32

} // namespace facebook::eden
