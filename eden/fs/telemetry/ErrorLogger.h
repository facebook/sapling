/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/fs/telemetry/EdenStructuredLogger.h"

namespace facebook::eden {

class EdenErrorInfoBuilder;
class ReloadableConfig;
class ScribeLogger;

// Logs structured error events to perfpipe_edenfs_errors for error telemetry.
class ErrorLogger : public EdenStructuredLogger {
 public:
  ErrorLogger(
      std::shared_ptr<ScribeLogger> scribeLogger,
      SessionInfo sessionInfo,
      std::shared_ptr<ReloadableConfig> config);

  ~ErrorLogger() override = default;

  /**
   * Log a structured error event to perfpipe_edenfs_errors.
   * Must be called promptly from a catch block — the throw-site trace
   * is in thread-local storage and will be overwritten by the next
   * throw on this thread.
   * Example:
   *   logger->logEvent(
   *       EdenErrorInfo::takeover(ex).withMountPoint(path));
   *
   * Stack trace symbolization, Manifold upload, and Scribe send only
   * happen when enableErrorLogging is true. When off, returns with
   * zero cost.
   */
  void logEvent(EdenErrorInfoBuilder builder);

  bool isEnabled() const;

 private:
  std::shared_ptr<ReloadableConfig> config_;
};

} // namespace facebook::eden
