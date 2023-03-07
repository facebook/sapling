/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/StructuredLoggerFactory.h"
#include <fb303/ServiceData.h>
#include <folly/logging/xlog.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/telemetry/ScubaStructuredLogger.h"
#include "eden/fs/telemetry/SubprocessScribeLogger.h"

namespace facebook::eden {

std::shared_ptr<StructuredLogger> makeDefaultStructuredLogger(
    const EdenConfig& config,
    SessionInfo sessionInfo,
    std::shared_ptr<EdenStats> edenStats) {
  const auto& binary = config.scribeLogger.getValue();
  const auto& category = config.scribeCategory.getValue();

  if (binary.empty()) {
    return std::make_shared<NullStructuredLogger>();
  }

  if (category.empty()) {
    XLOGF(
        WARN,
        "Scribe binary '{}' specified, but no category specified. Structured logging is disabled.",
        binary);
    return std::make_shared<NullStructuredLogger>();
  }

  try {
    auto logger =
        std::make_unique<SubprocessScribeLogger>(binary.c_str(), category);
    return std::make_shared<ScubaStructuredLogger>(
        std::move(logger), std::move(sessionInfo));
  } catch (const std::exception& ex) {
    edenStats->increment(&TelemetryStats::subprocessLoggerFailure, 1);
    XLOGF(
        ERR,
        "Failed to create SubprocessScribeLogger: {}. Structured logging is disabled.",
        folly::exceptionStr(ex));
    return std::make_shared<NullStructuredLogger>();
  }
}

} // namespace facebook::eden
