/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ErrorLogger.h"

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/DaemonError.h"
#include "eden/fs/telemetry/EdenErrorInfoBuilder.h"
#include "eden/fs/telemetry/StackTraceUploader.h"

namespace facebook::eden {

ErrorLogger::ErrorLogger(
    std::shared_ptr<ScribeLogger> scribeLogger,
    SessionInfo sessionInfo,
    std::shared_ptr<ReloadableConfig> config)
    : EdenStructuredLogger(std::move(scribeLogger), std::move(sessionInfo)),
      config_(std::move(config)) {}

bool ErrorLogger::isEnabled() const {
  return config_->getEdenConfig()->enableErrorLogging.getValue();
}

void ErrorLogger::logEvent(EdenErrorInfoBuilder builder) {
  auto edenConfig = config_->getEdenConfig();
  if (!edenConfig->enableErrorLogging.getValue()) {
    return;
  }
  auto event = builder.createEvent();
  if (event.info.stackTrace.has_value() &&
      edenConfig->enableStackTraceUpload.getValue()) {
    event.info.stackTrace =
        StackTraceUploader::uploadToManifold(std::move(*event.info.stackTrace));
  }
  EdenStructuredLogger::logEvent(std::move(event));
}

} // namespace facebook::eden
