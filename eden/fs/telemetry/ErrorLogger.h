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

class ErrorLogger {
 public:
  ErrorLogger(
      std::shared_ptr<ScribeLogger> scribeLogger,
      SessionInfo sessionInfo,
      std::shared_ptr<ReloadableConfig> config);

  /**
   * Log a structured error event to perfpipe_edenfs_errors.
   * Must be called promptly from a catch block — the throw-site trace
   * is in thread-local storage and will be overwritten by the next
   * throw on this thread.
   *
   * Stack trace symbolization, Manifold upload, and Scribe send only
   * happen when scribe is configured and enableErrorLogging is true.
   * Otherwise, returns with zero cost.
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
};

} // namespace facebook::eden
