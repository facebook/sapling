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

void ErrorLogger::log(EdenErrorInfoBuilder builder) {
  if (!config_ || !xplatLogger_) {
    return;
  }
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

  if (edenStats_) {
    edenStats_->increment(&TelemetryStats::errorsViaXplatLogger);
  }
  DynamicEvent de;
  event.populate(de);
  xplatLogger_->logEvent(xplat_keys::kErrorsCategory, de);
}

} // namespace facebook::eden
