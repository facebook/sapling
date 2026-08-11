/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/common/telemetry/LogEvent.h"

namespace facebook::eden {

class IXplatLogger;

/**
 * Logs edenfs_events telemetry through the process-wide XplatLogger.
 *
 * The shared pointer keeps the logger alive for asynchronous owners of this
 * facade. Logging is a no-op when no XplatLogger implementation is available.
 * TypedEvent adds the event's type field; TypelessEvent omits it.
 */
class EdenFsEventsLogger {
 public:
  explicit EdenFsEventsLogger(std::shared_ptr<IXplatLogger> xplatLogger);

  void logEvent(const TypedEvent& event) const;
  void logEvent(const TypelessEvent& event) const;

 private:
  std::shared_ptr<IXplatLogger> xplatLogger_;
};

} // namespace facebook::eden
