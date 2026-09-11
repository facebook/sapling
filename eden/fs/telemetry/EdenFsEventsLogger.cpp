/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenFsEventsLogger.h"

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/fs/telemetry/IXplatLogger.h"
#include "eden/fs/telemetry/XplatKeys.h"

namespace facebook::eden {

EdenFsEventsLogger::EdenFsEventsLogger(
    std::shared_ptr<IXplatLogger> xplatLogger)
    : xplatLogger_{std::move(xplatLogger)} {}

void EdenFsEventsLogger::logEvent(const TypedEvent& event) const {
  DynamicEvent de;
  event.populate(de);
  de.addString(std::string(xplat_keys::kType), std::string(event.getType()));
  logEvent(de);
}

void EdenFsEventsLogger::logEvent(const TypelessEvent& event) const {
  DynamicEvent de;
  event.populate(de);
  logEvent(de);
}

void EdenFsEventsLogger::logEvent(const DynamicEvent& event) const {
  if (xplatLogger_) {
    xplatLogger_->logEvent(xplat_keys::kEventsCategory, event);
  }
}

} // namespace facebook::eden
