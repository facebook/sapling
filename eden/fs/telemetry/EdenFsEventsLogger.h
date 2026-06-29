/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/common/telemetry/LogEvent.h"
#include "eden/common/utils/RefPtr.h"

namespace facebook::eden {

class EdenStats;
class IXplatLogger;
class ReloadableConfig;
class StructuredLogger;

using EdenStatsPtr = RefPtr<EdenStats>;

/**
 * EdenFsEventsLogger is a temporary wrapper that gates between the legacy
 * StructuredLogger path and the new XplatLogger Thrift path during the
 * XplatLogger migration. Once all call sites are migrated to XplatLogger
 * and the migration is validated, this wrapper can be removed and callers
 * can use XplatLogger directly.
 *
 * Uses either/or pattern: when XplatLogger is available and config flag is
 * enabled, logs to XplatLogger. Otherwise, logs to StructuredLogger.
 * TypedEvent overload adds the type field to the XplatLogger event;
 * TypelessEvent overload omits it.
 */
class EdenFsEventsLogger {
 public:
  EdenFsEventsLogger(
      std::shared_ptr<StructuredLogger> structuredLogger,
      IXplatLogger* xplatLogger,
      std::shared_ptr<ReloadableConfig> reloadableConfig,
      EdenStatsPtr edenStats);

  void logEvent(const TypedEvent& event);
  void logEvent(const TypelessEvent& event);

  const std::shared_ptr<StructuredLogger>& getStructuredLogger() const {
    return structuredLogger_;
  }

 private:
  std::shared_ptr<StructuredLogger> structuredLogger_;
  IXplatLogger* xplatLogger_;
  std::shared_ptr<ReloadableConfig> reloadableConfig_;
  EdenStatsPtr edenStats_;
};

} // namespace facebook::eden
