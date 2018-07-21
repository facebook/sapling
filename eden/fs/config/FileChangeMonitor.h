/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <sys/stat.h>
#include <chrono>
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * FileChangeMonitor monitors a file for changes. Its provides an interface
 * "changedSinceUpdate()" to check if the monitored file has changed since
 * the last time "updateStat()" was called.
 * Typical usage:
 * - fcm.changedSinceUpdate() to determine if the monitored file changed.
 * - if fcm.changedSinceUpdate() = TRUE, process the updated file.
 * Then, call fcm.updateStat() (so that subsequent changedSinceUpdate() calls
 * will be based on the updated file.
 *
 * FileChangeMonitor limits resource usage by checking for changes on-demand
 * and by throttling. The throtte limits checks to 1 per throttleMilliSeconds.
 *
 * FileChangeMonitor is not thread safe - users are responsible for locking as
 * necessary.
 */
class FileChangeMonitor {
 public:
  /**
   * Construct a FileChangeMonitor for the provided filePath.
   * @param throttleMilliSeconds specifies minimum time between file stats.
   */
  FileChangeMonitor(
      AbsolutePathPiece filePath,
      std::chrono::milliseconds throttleMilliSeconds)
      : filePath_{filePath}, throttleMilliSeconds_{throttleMilliSeconds} {
    resetToForceChange();
  }

  ~FileChangeMonitor() = default;

  FileChangeMonitor(const FileChangeMonitor& fcm) = default;

  FileChangeMonitor(FileChangeMonitor&& fcm) = default;

  FileChangeMonitor& operator=(const FileChangeMonitor& fcm) = default;

  FileChangeMonitor& operator=(FileChangeMonitor&& fcm) = default;

  /**
   * Perform stat to determine if the file has changed since the last
   * updateStat(). The first call to this method is always TRUE.
   * @return false if the check is throttled or the file has not changed. True,
   * if stat fails, or the file has changed (since last stat update).
   */
  bool changedSinceUpdate(bool noThrottle = false);

  /**
   * Change the path of the monitored file. Resets stat and lastCheck_ to force
   * the next "changedSinceUpdate()" to be TRUE.
   */
  void setFilePath(AbsolutePathPiece filePath);

  /**
   * @return the monitored file path.
   */
  AbsolutePath getFilePath();

  /**
   * Update the fileStat by doing a stat using the passed fileDescriptor.
   * changedSinceUpdate() will determine changes based on the updated fileStat.
   * If the fileDescriptor is invalid (< 0) we update consistent with
   * non-existing file.
   */
  void updateStat(int fileDescriptor);

  /**
   * Update the fileStat with the passed error (eg. from stat or open)
   * changedSinceUpdate() will determine changes based on the updated fileStat.
   */
  void updateStatWithError(int errorNum);

 private:
  /**
   * @return TRUE if the time elapsed since last call is less than
   * throttleMilliSeconds.
   */
  bool throttle();

  /**
   * Reset to base state - next call to changedSinceUpdate will return true
   * (requires throttle to not activate). Useful during initialization and if
   * the monitored file's path has changed.
   */
  void resetToForceChange() {
    // Set values for stat to force changedSinceUpdate() to return TRUE.
    // We use a novel setting to force change to be detected
    memset(&fileStat_, 0, sizeof(struct stat));
    fileStat_.st_mtim.tv_sec = 1;
    fileStat_.st_mtim.tv_nsec = 1;
    statErrno_ = 0;
    // Set lastCheck in past so throttle does not apply.
    lastCheck_ = std::chrono::steady_clock::now() - throttleMilliSeconds_ -
        std::chrono::seconds{1};
  }

  AbsolutePath filePath_;
  struct stat fileStat_;
  int statErrno_{0};
  std::chrono::milliseconds throttleMilliSeconds_;
  std::chrono::steady_clock::time_point lastCheck_;
};
} // namespace eden
} // namespace facebook
