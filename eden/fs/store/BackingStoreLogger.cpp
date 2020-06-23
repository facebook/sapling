/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/BackingStoreLogger.h"

#include <folly/Conv.h>

#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/ProcessNameCache.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

namespace facebook {
namespace eden {

BackingStoreLogger::BackingStoreLogger(
    std::shared_ptr<StructuredLogger> logger,
    std::shared_ptr<ProcessNameCache> processNameCache)
    : logger_{std::move(logger)},
      processNameCache_{std::move(processNameCache)},
      loggingAvailable_{true} {}

void BackingStoreLogger::logImport(
    ObjectFetchContext& context,
    RelativePathPiece importPath) {
  if (!loggingAvailable_) {
    return;
  }
  auto pid = context.getClientPid();
  auto cause = context.getCause();
  auto importPathString = importPath.stringPiece().str();

  std::optional<std::string> cmdline;
  if (pid) {
    cmdline = processNameCache_->getProcessName(pid.value());
  }

  std::string cause_string = "<invalid>";
  switch (cause) {
    case ObjectFetchContext::Cause::Fuse:
      cause_string = "FUSE";
      break;
    case ObjectFetchContext::Cause::Thrift:
      cause_string = "Thrift";
      break;
    case ObjectFetchContext::Unknown:
      cause_string = "Unknown";
  }

  logger_->logEvent(ServerDataFetch{std::move(cause_string),
                                    pid,
                                    std::move(cmdline),
                                    std::move(importPathString)});
}

} // namespace eden
} // namespace facebook
