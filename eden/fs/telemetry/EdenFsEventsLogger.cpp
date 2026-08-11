/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenFsEventsLogger.h"

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/IXplatLogger.h"
#include "eden/fs/telemetry/XplatKeys.h"

namespace facebook::eden {

EdenFsEventsLogger::EdenFsEventsLogger(
    std::shared_ptr<IXplatLogger> xplatLogger)
    : xplatLogger_{std::move(xplatLogger)} {}

EdenFsEventsLogger::EdenFsEventsLogger(
    std::shared_ptr<StructuredLogger> /*structuredLogger*/,
    std::shared_ptr<IXplatLogger> xplatLogger,
    std::shared_ptr<ReloadableConfig> /*reloadableConfig*/,
    EdenStatsPtr /*edenStats*/)
    : EdenFsEventsLogger{std::move(xplatLogger)} {}

void EdenFsEventsLogger::logEvent(const TypedEvent& event) const {
  if (!xplatLogger_) {
    return;
  }
  DynamicEvent de;
  event.populate(de);
  de.addString(std::string(xplat_keys::kType), std::string(event.getType()));
  xplatLogger_->logEvent(xplat_keys::kEventsCategory, de);
}

void EdenFsEventsLogger::logEvent(const TypelessEvent& event) const {
  if (!xplatLogger_) {
    return;
  }
  DynamicEvent de;
  event.populate(de);
  xplatLogger_->logEvent(xplat_keys::kEventsCategory, de);
}

} // namespace facebook::eden
