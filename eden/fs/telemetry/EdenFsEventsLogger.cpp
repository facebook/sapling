/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenFsEventsLogger.h"

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/common/telemetry/Stats.h"
#include "eden/common/telemetry/StructuredLogger.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/XplatKeys.h"
#include "eden/fs/telemetry/facebook/XplatLogger.h"

namespace facebook::eden {

EdenFsEventsLogger::EdenFsEventsLogger(
    std::shared_ptr<StructuredLogger> structuredLogger,
    XplatLogger* xplatLogger,
    std::shared_ptr<ReloadableConfig> reloadableConfig,
    EdenStatsPtr edenStats)
    : structuredLogger_(std::move(structuredLogger)),
      xplatLogger_(xplatLogger),
      reloadableConfig_(std::move(reloadableConfig)),
      edenStats_(std::move(edenStats)) {}

void EdenFsEventsLogger::logEvent(const TypedEvent& event) {
  // Either/or pattern: XplatLogger path OR StructuredLogger path
  if (xplatLogger_ && reloadableConfig_ &&
      reloadableConfig_->getEdenConfig()->enableXplatLoggerEvents.getValue()) {
    edenStats_->increment(&TelemetryStats::eventsViaXplatLogger);
    DynamicEvent de;
    event.populate(de);
    de.addString(std::string(xplat_keys::kType), std::string(event.getType()));
    xplatLogger_->logEvent(xplat_keys::kEventsCategory, de);
  } else {
    edenStats_->increment(&TelemetryStats::eventsViaStructuredLogger);
    structuredLogger_->logEvent(event);
  }
}

void EdenFsEventsLogger::logEvent(const TypelessEvent& event) {
  // Either/or pattern: XplatLogger path OR StructuredLogger path
  if (xplatLogger_ && reloadableConfig_ &&
      reloadableConfig_->getEdenConfig()->enableXplatLoggerEvents.getValue()) {
    edenStats_->increment(&TelemetryStats::eventsViaXplatLogger);
    DynamicEvent de;
    event.populate(de);
    xplatLogger_->logEvent(xplat_keys::kEventsCategory, de);
  } else {
    edenStats_->increment(&TelemetryStats::eventsViaStructuredLogger);
    structuredLogger_->logEvent(event);
  }
}

} // namespace facebook::eden
