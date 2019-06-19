/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/File.h>
#include <sys/stat.h>
#include <chrono>
#include <functional>
#include <optional>

#ifdef _WIN32
#include "eden/fs/win/utils/Stub.h" //@manual
#endif

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * Function to check if the passed stats are equal or not.
 */
bool equalStats(const struct stat& stat1, const struct stat& stat2) noexcept;

/**
 * FileChangeMonitor monitors a file for changes. The "invokeIfUpdated()"
 * method initiates a check and, if the file has changed, will run the
 * provided call-back method. Typical usage:
 * - construct, a FileChangeMonitor eg. FileChangeMonitor fcm{path, 4s}
 * - periodically call invokeIfUpdated() to check for changes and potentially
 * run the call-back.
 *
 * FileChangeMonitor performs checks on demand. The throttleDuration setting
 * can further limit resource usage (to a maximum of 1 check/throttleDuration).
 *
 * FileChangeMonitor is not thread safe - users are responsible for locking as
 * necessary.
 */
class FileChangeMonitor {
 public:
  /**
   * Construct a FileChangeMonitor for the provided filePath.
   * @param throttleDuration specifies minimum time between file stats.
   */
  FileChangeMonitor(
      AbsolutePathPiece filePath,
      std::chrono::milliseconds throttleDuration)
      : filePath_{filePath}, throttleDuration_{throttleDuration} {
    resetToForceChange();
  }

  ~FileChangeMonitor() = default;

  FileChangeMonitor(const FileChangeMonitor& fcm) = default;

  FileChangeMonitor(FileChangeMonitor&& fcm) = default;

  FileChangeMonitor& operator=(const FileChangeMonitor& fcm) = default;

  FileChangeMonitor& operator=(FileChangeMonitor&& fcm) = default;

  /**
   * Check if the monitored file has been updated in a meaningful way.
   * Meaningful changes include:
   * - file modifications where file can be opened;
   * - file modifications, where file cannot be opened AND its stat or open
   * error code has changed;
   * We suppress file change notifications where the same error exists. For
   * example, no call-back will be issued for a file that has changed but, open
   * still returns EACCES.
   */
  std::optional<folly::Expected<folly::File, int>> checkIfUpdated(
      bool noThrottle = false);

  /**
   * If the monitored file has changed in a "meaningful" way, the
   * FileChangeProcessor call-back will be invoked. The call-back will always
   * be invoked on the first call. This method will not catch exceptions
   * thrown by the call-back.
   * @ see checkIfUpdated for details on "meaningful" changes.
   * @return TRUE if the call-back was invoked, else, FALSE.
   */
  template <typename Fn>
  bool invokeIfUpdated(Fn&& fn, bool noThrottle = false) {
    auto result = checkIfUpdated(noThrottle);
    if (!result.has_value()) {
      return false;
    }
    if (result.value().hasValue()) {
      fn(std::move(result.value()).value(), 0, filePath_);
    } else {
      fn(std::move(folly::File()), result->error(), filePath_);
    }
    return true;
  }

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
   * isChanged(). It updates statErrno_ which can be used to by invokeIfUpdated
   * to make optimizations.
   */
  bool isChanged();

  /**
   * Reset to base state. The next call to isChanged will return true
   * (this requires throttle to NOT activate). Useful during initialization and
   * if the monitored file's path has changed.
   */
  void resetToForceChange() {
    // Set values for stat to force changedSinceUpdate() to return TRUE.
    // We use a novel setting to force change to be detected
    memset(&fileStat_, 0, sizeof(struct stat));
    fileStat_.st_mtime = 1;

    statErrno_ = 0;
    openErrno_ = 0;
    // Set lastCheck in past so throttle does not apply.
    lastCheck_ = std::chrono::steady_clock::now() - throttleDuration_ -
        std::chrono::seconds{1};
  }

  AbsolutePath filePath_;
  struct stat fileStat_;
  int statErrno_{0};
  int openErrno_{0};
  std::chrono::milliseconds throttleDuration_;
  std::chrono::steady_clock::time_point lastCheck_;
};
} // namespace eden
} // namespace facebook
