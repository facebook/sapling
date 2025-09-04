/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include <string>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/service/EdenStateDir.h"

namespace facebook::eden {

class StructuredLogger;

/**
 * Manages heartbeat files for Eden daemon processes.
 *
 * The heartbeat logic:
 * - Write a heartbeat file when Eden starts
 * - Update it periodically while running
 * - Delete it on clean shutdown
 * - Check for previous heartbeat files on startup to detect crashes
 */
class HeartbeatManager {
 public:
  explicit HeartbeatManager(
      const EdenStateDir& edenDir,
      std::shared_ptr<StructuredLogger> structuredLogger);

  ~HeartbeatManager() = default;

  // Non-copyable, non-movable
  HeartbeatManager(const HeartbeatManager&) = delete;
  HeartbeatManager& operator=(const HeartbeatManager&) = delete;
  HeartbeatManager(HeartbeatManager&&) = delete;
  HeartbeatManager& operator=(HeartbeatManager&&) = delete;

  /**
   * Create or update the heartbeat file with current timestamp
   */
  void createOrUpdateHeartbeatFile();

  /**
   * Remove the heartbeat file for clean shutdown
   */
  void removeHeartbeatFile();

  /**
   * Check for previous heartbeat files and handle crash detection.
   * Should be called during startup.
   *
   * @param takeover Whether this is a takeover operation
   * @param oldDaemonPid Optional PID of old daemon (for takeover)
   * @return True if a crash was detected
   */
  bool checkForPreviousHeartbeat(
      bool takeover,
      const std::optional<std::string>& oldEdenHeartbeatFileNameStr =
          std::nullopt);

#ifndef _WIN32
  /**
   * Create a daemon exit signal file with the given signal number.
   * This is called from signal handlers and must be async-signal-safe.
   */
  void createDaemonExitSignalFile(int signal);

  /**
   * Remove the daemon exit signal file
   */
  void removeDaemonExitSignalFile();
#endif

  /**
   * Read the signal number from the daemon exit signal file
   * @return Signal number, or 0 if file doesn't exist or is invalid
   */
  int readDaemonExitSignal();

  /**
   * Get the heartbeat file name for the current process
   */
  std::string getHeartbeatFileName() const;

 private:
  const EdenStateDir& edenDir_;
  std::shared_ptr<StructuredLogger> structuredLogger_;

  // Cached paths for performance
  AbsolutePath heartbeatFilePath_;
  std::string heartbeatFilePathString_;
  AbsolutePath daemonExitSignalFilePath_;
  std::string daemonExitSignalFilePathString_;

  /**
   * Helper function to convert integer to string in async-signal-safe way
   */
  static int intToStrSafe(int val, char* buf, size_t buf_size);
};

} // namespace facebook::eden
