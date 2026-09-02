/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/privhelper/PrivHelper.h"

#include <folly/Conv.h>
#include <folly/File.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <cstdlib>

namespace facebook::eden {

uint64_t readEdenFsRestartCounterEnv(folly::StringPiece name) {
  const char* value = std::getenv(name.str().c_str());
  if (value == nullptr || *value == '\0') {
    return 0;
  }
  const auto parsed = folly::tryTo<uint64_t>(folly::StringPiece{value});
  if (parsed.hasError()) {
    XLOGF(WARN, "ignoring unparsable {}={}", name, value);
    return 0;
  }
  return parsed.value();
}

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

folly::Future<folly::Unit> PrivHelper::setRestartArgs(
    const EdenFsRestartArgs& /* args */) {
  return folly::makeFuture();
}

void PrivHelper::notifyCleanShutdown(folly::StringPiece /* reason */) noexcept {
}

NamespaceInfo PrivHelper::getNamespaceInfoBlocking(pid_t daemonPid) {
  folly::EventBase evb;
  attachEventBase(&evb);

  auto future = getNamespaceInfo(daemonPid);
  if (future.isReady()) {
    return std::move(future).get();
  }

  future = std::move(future).ensure([&evb] { evb.terminateLoopSoon(); });
  evb.loopForever();
  return std::move(future).get();
}

} // namespace facebook::eden
