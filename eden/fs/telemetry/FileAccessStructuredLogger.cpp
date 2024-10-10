/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/FileAccessStructuredLogger.h"

namespace facebook::eden {

FileAccessStructuredLogger::FileAccessStructuredLogger(
    std::shared_ptr<ScribeLogger> scribeLogger,
    SessionInfo sessionInfo)
    : EdenStructuredLogger{std::move(scribeLogger), std::move(sessionInfo)} {}

DynamicEvent FileAccessStructuredLogger::populateDefaultFields(
    std::optional<const char*> type) {
  return EdenStructuredLogger::populateDefaultFields(type);
}

} // namespace facebook::eden
