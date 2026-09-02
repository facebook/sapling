/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/common/utils/RefPtr.h"

namespace facebook::eden {

class EdenErrorInfoBuilder;
class EdenStats;
class IXplatLogger;
class ReloadableConfig;

using EdenStatsPtr = RefPtr<EdenStats>;

class ErrorLogger {
 public:
  explicit ErrorLogger(
      std::shared_ptr<ReloadableConfig> config = nullptr,
      IXplatLogger* xplatLogger = nullptr,
      EdenStatsPtr edenStats = nullptr);

  /**
   * Log a structured error event.
   *
   * Must be called promptly from a catch block — the throw-site trace
   * is in thread-local storage and will be overwritten by the next
   * throw on this thread.
   *
   * Requires enableErrorLogging to be true and an XplatLogger to be available.
   * The event is sent to GeneratedEdenfsErrorsLoggerConfig (Hive + Scuba).
   *
   * Stack trace symbolization and Manifold upload happen only when
   * enableStackTraceUpload is true. If error logging is disabled or XplatLogger
   * is unavailable, returns with zero cost.
   *
   * Example:
   *   logger->log(EdenErrorInfo::fuse(ex, ino, mountPath));
   */
  void log(EdenErrorInfoBuilder builder);

  bool isEnabled() const;

 private:
  std::shared_ptr<ReloadableConfig> config_;
  // Not owned; outlives ErrorLogger (owned by EdenServer). May be null when
  // the XplatLogger is unavailable (e.g. EDEN_HAVE_LOGGER is off, or tests).
  IXplatLogger* xplatLogger_;
  EdenStatsPtr edenStats_;
};

} // namespace facebook::eden
