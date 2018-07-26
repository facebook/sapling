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
#include <functional>
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class File;
};

namespace facebook {
namespace eden {

/**
 * FileChangeMonitor monitors a file for changes. The "invokeIfUpdated()"
 * method initiates a check. If the file has changed, it will run the
 * FileChangeProcessor call-back method. Typical usage:
 * - construct, FileChangeMonitor with FileChangeProcessor to handle call-backs
 *   eg. FileChangeMonitor fcm{path, 4s, cb}
 * - Periodically call invokeIfUpdated() to initiate check/call-back on file
 *   changes.
 *
 * FileChangeMonitor performs checks on demand. The throttleDuration setting
 * can further limit resource usage (to a maximum of 1 check/throttleDuration).
 *
 * FileChangeMonitor is not thread safe - users are responsible for locking as
 * necessary.
 */
class FileChangeMonitor {
 public:
  /** The FileChangeProcessor is a call-back function for file changes.
   * If stat or open failed, the errorNum will be set, otherwise, the file can
   * be used.
   */
  using FileChangeProcessor = std::function<
      void(folly::File&& f, int errorNum, AbsolutePathPiece filePath)>;

  /**
   * Construct a FileChangeMonitor for the provided filePath.
   * @param throttleDuration specifies minimum time between file stats.
   * @param fileChangeProcessor will be called when the file is changed.
   */
  FileChangeMonitor(
      AbsolutePathPiece filePath,
      std::chrono::milliseconds throttleDuration,
      FileChangeProcessor fileChangeProcessor)
      : filePath_{filePath},
        throttleDuration_{throttleDuration},
        fileChangeProcessor_{fileChangeProcessor} {
    resetToForceChange();
  }

  ~FileChangeMonitor() = default;

  FileChangeMonitor(const FileChangeMonitor& fcm) = default;

  FileChangeMonitor(FileChangeMonitor&& fcm) = default;

  FileChangeMonitor& operator=(const FileChangeMonitor& fcm) = default;

  FileChangeMonitor& operator=(FileChangeMonitor&& fcm) = default;

  /**
   * Check if the file has been change by doing a stat. If it has changed, the
   * FileChangeProcessor call-back will be invoked. The call-back will always
   * be invoked on the first call. This method will not catch exceptions
   * thrown by the call-back.
   * @return TRUE if the call-back was invoked, else, FALSE.
   */
  bool invokeIfUpdated(bool noThrottle = false);

  /**
   * Change the path of the monitored file. Resets stat and lastCheck_ to force
   * the next "invokeIfUpdated()" to be TRUE.
   */
  void setFilePath(AbsolutePathPiece filePath);

  /**
   * @return the monitored file path.
   */
  AbsolutePath getFilePath();

 private:
  /**
   * @return TRUE if the time elapsed since last call is less than
   * throttleDuration.
   */
  bool throttle();

  /** Stat the monitored file and compare results against last call to
   * isChanged(). It updates fileStat_ and statErrno_.
   */
  bool isChanged();

  /**
   * Reset to the base state. The next call to isChanged will return true
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
    lastCheck_ = std::chrono::steady_clock::now() - throttleDuration_ -
        std::chrono::seconds{1};
  }

  AbsolutePath filePath_;
  struct stat fileStat_;
  int statErrno_{0};
  std::chrono::milliseconds throttleDuration_;
  std::chrono::steady_clock::time_point lastCheck_;
  FileChangeProcessor fileChangeProcessor_;
};
} // namespace eden
} // namespace facebook
