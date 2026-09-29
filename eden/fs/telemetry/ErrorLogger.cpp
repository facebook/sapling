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
#include "eden/fs/telemetry/EdenComponent.h"
#include "eden/fs/telemetry/EdenErrorInfoBuilder.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/IXplatLogger.h"
#include "eden/fs/telemetry/StackTraceUploader.h"
#include "eden/fs/telemetry/XplatKeys.h"

namespace facebook::eden {

ErrorLogger::ErrorLogger(
    std::shared_ptr<ReloadableConfig> config,
    IXplatLogger* xplatLogger,
    EdenStatsPtr edenStats)
    : config_(std::move(config)),
      xplatLogger_(xplatLogger),
      edenStats_(std::move(edenStats)) {}

bool ErrorLogger::isEnabled() const {
  return config_ && xplatLogger_ &&
      config_->getEdenConfig()->enableErrorLogging.getValue();
}

ErrorLogOutcome ErrorLogger::log(EdenErrorInfoBuilder builder) {
  if (!config_) {
    return ErrorLogOutcome::Disabled;
  }
  auto edenConfig = config_->getEdenConfig();

  std::string_view key = builder.errorType().has_value()
      ? std::string_view{*builder.errorType()}
      : toString(builder.component());
  auto suppressed = rateLimiter_.tryAcquire(
      key,
      edenConfig->errorLogMaxPerMinute.getValue() / 60.0,
      edenConfig->errorLogBurst.getValue());
  if (!suppressed.has_value()) {
    if (edenStats_) {
      edenStats_->increment(&TelemetryStats::errorsRateLimited);
    }
    return ErrorLogOutcome::RateLimited;
  }

  if (!xplatLogger_ || !edenConfig->enableErrorLogging.getValue()) {
    return ErrorLogOutcome::Disabled;
  }

  auto event = builder.createEvent();
  if (*suppressed > 0) {
    event.info.suppressedCount = *suppressed;
  }
  if (event.info.stackTrace.has_value() &&
      edenConfig->enableStackTraceUpload.getValue()) {
    event.info.stackTrace =
        StackTraceUploader::uploadToManifold(std::move(*event.info.stackTrace));
  }

  if (edenStats_) {
    edenStats_->increment(&TelemetryStats::errorsViaXplatLogger);
  }
  DynamicEvent de;
  event.populate(de);
  xplatLogger_->logEvent(xplat_keys::kErrorsCategory, de);
  return ErrorLogOutcome::Logged;
}

} // namespace facebook::eden
