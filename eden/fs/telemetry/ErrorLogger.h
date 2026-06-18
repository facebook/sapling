/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/common/utils/RefPtr.h"
#include "eden/fs/telemetry/EdenStructuredLogger.h"

namespace facebook::eden {

class EdenErrorInfoBuilder;
class EdenStats;
class ReloadableConfig;
class ScribeLogger;
class XplatLogger;

using EdenStatsPtr = RefPtr<EdenStats>;

class ErrorLogger {
 public:
  ErrorLogger(
      std::shared_ptr<ScribeLogger> scribeLogger,
      SessionInfo sessionInfo,
      std::shared_ptr<ReloadableConfig> config,
      XplatLogger* xplatLogger = nullptr,
      EdenStatsPtr edenStats = nullptr);

  /**
   * Log a structured error event.
   *
   * Must be called promptly from a catch block — the throw-site trace
   * is in thread-local storage and will be overwritten by the next
   * throw on this thread.
   *
   * Requires enableErrorLogging to be true. The event is then routed one of
   * two ways:
   *   - XplatLogger path (when enableXplatLoggerErrors is true and an
   *     XplatLogger is available): sent to GeneratedEdenfsErrorsLoggerConfig
   *     (Hive + Scuba). Does not require a scribe binary.
   *   - Legacy path (otherwise): sent via ScribeLogger to
   *     perfpipe_edenfs_errors, which requires a scribe binary to be
   *     configured.
   *
   * Stack trace symbolization and Manifold upload happen for whichever path
   * is taken, and only when enableStackTraceUpload is true. If error logging
   * is disabled or neither path is available, returns with zero cost.
   *
   * Example:
   *   logger->log(EdenErrorInfo::fuse(ex, ino, mountPath));
   */
  void log(EdenErrorInfoBuilder builder);

  bool isEnabled() const;

 private:
  bool hasScribe_;
  EdenStructuredLogger structuredLogger_;
  std::shared_ptr<ReloadableConfig> config_;
  // Not owned; outlives ErrorLogger (owned by EdenServer). May be null when
  // the XplatLogger is unavailable (e.g. EDEN_HAVE_LOGGER is off, or tests).
  XplatLogger* xplatLogger_;
  EdenStatsPtr edenStats_;
};

} // namespace facebook::eden
