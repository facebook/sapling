/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/StructuredLoggerFactory.h"
#include <folly/logging/xlog.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/telemetry/ScubaStructuredLogger.h"
#include "eden/fs/telemetry/SubprocessScribeLogger.h"

namespace facebook::eden {

std::shared_ptr<StructuredLogger> makeDefaultStructuredLogger(
    const EdenConfig& config,
    SessionInfo sessionInfo) {
  const auto& binary = config.scribeLogger.getValue();
  const auto& category = config.scribeCategory.getValue();

  if (binary.empty()) {
    return std::make_shared<NullStructuredLogger>();
  }

  if (category.empty()) {
    XLOG(WARN)
        << "Scribe binary specified, but no category specified. Structured logging is disabled.";
    return std::make_shared<NullStructuredLogger>();
  }

  auto logger =
      std::make_unique<SubprocessScribeLogger>(binary.c_str(), category);
  return std::make_shared<ScubaStructuredLogger>(
      std::move(logger), std::move(sessionInfo));
}

} // namespace facebook::eden
