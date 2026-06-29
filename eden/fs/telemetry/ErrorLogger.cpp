/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ErrorLogger.h"

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/common/telemetry/Stats.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/DaemonError.h"
#include "eden/fs/telemetry/EdenErrorInfoBuilder.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/IXplatLogger.h"
#include "eden/fs/telemetry/StackTraceUploader.h"
#include "eden/fs/telemetry/XplatKeys.h"

namespace facebook::eden {

ErrorLogger::ErrorLogger(
    std::shared_ptr<ScribeLogger> scribeLogger,
    SessionInfo sessionInfo,
    std::shared_ptr<ReloadableConfig> config,
    IXplatLogger* xplatLogger,
    EdenStatsPtr edenStats)
    : hasScribe_(scribeLogger != nullptr),
      structuredLogger_(std::move(scribeLogger), std::move(sessionInfo)),
      config_(std::move(config)),
      xplatLogger_(xplatLogger),
      edenStats_(std::move(edenStats)) {}

bool ErrorLogger::isEnabled() const {
  if (!config_ || !config_->getEdenConfig()->enableErrorLogging.getValue()) {
    return false;
  }
  const bool useXplat = xplatLogger_ &&
      config_->getEdenConfig()->enableXplatLoggerErrors.getValue();
  return hasScribe_ || useXplat;
}

void ErrorLogger::log(EdenErrorInfoBuilder builder) {
  if (!config_) {
    return;
  }
  auto edenConfig = config_->getEdenConfig();
  if (!edenConfig->enableErrorLogging.getValue()) {
    return;
  }

  // Either/or routing: when enabled and available, log to the XplatLogger
  // (GeneratedEdenfsErrorsLoggerConfig -> Hive + Scuba). Otherwise fall back to
  // the legacy Scribe -> perfpipe_edenfs_errors path, which needs a scribe
  // binary configured.
  const bool useXplat =
      xplatLogger_ && edenConfig->enableXplatLoggerErrors.getValue();
  if (!useXplat && !hasScribe_) {
    return;
  }

  auto event = builder.createEvent();
  if (event.info.stackTrace.has_value() &&
      edenConfig->enableStackTraceUpload.getValue()) {
    event.info.stackTrace =
        StackTraceUploader::uploadToManifold(std::move(*event.info.stackTrace));
  }

  if (useXplat) {
    if (edenStats_) {
      edenStats_->increment(&TelemetryStats::errorsViaXplatLogger);
    }
    DynamicEvent de;
    event.populate(de);
    xplatLogger_->logEvent(xplat_keys::kErrorsCategory, de);
  } else {
    if (edenStats_) {
      edenStats_->increment(&TelemetryStats::errorsViaStructuredLogger);
    }
    structuredLogger_.logEvent(std::move(event));
  }
}

} // namespace facebook::eden
