/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <folly/logging/xlog.h>

#include "eden/fs/config/FileChangeMonitor.h"

namespace facebook {
namespace eden {

AbsolutePath FileChangeMonitor::getFilePath() {
  return filePath_;
}

void FileChangeMonitor::setFilePath(AbsolutePathPiece filePath) {
  if (filePath_ != filePath) {
    filePath_ = AbsolutePath{filePath};
    resetToForceChange();
  }
}

bool FileChangeMonitor::throttle() {
  auto rslt =
      (std::chrono::steady_clock::now() - lastCheck_) < throttleMilliSeconds_;
  return rslt;
}

void FileChangeMonitor::updateStatWithError(int errorNum) {
  statErrno_ = errorNum;
}

void FileChangeMonitor::updateStat(int fileDescriptor) {
  if (fileDescriptor < 0) {
    updateStatWithError(ENOENT);
    return;
  }
  statErrno_ = 0;
  auto rslt = fstat(fileDescriptor, &fileStat_);
  if (rslt != 0) {
    statErrno_ = errno;
    // Note: fstat, so we shouldn't get ENOENT
    XLOG(WARN) << "error accessing file " << filePath_ << ": "
               << folly::errnoStr(errno);
  }
}

bool FileChangeMonitor::changedSinceUpdate(bool noThrottle) {
  if (!noThrottle && throttle()) {
    return false;
  }

  // Update lastCheck - we use it for throttling
  lastCheck_ = std::chrono::steady_clock::now();
  int currentErrno{0};
  struct stat currentStat;

  // We are using stat to check for file deltas. Since we don't open file,
  // there is no chance of TOCTOU attack.
  int rslt = stat(filePath_.c_str(), &currentStat);
  // Log error if not ENOENT as they are unexpected and useful for debugging.
  // If error, return file change so that we update error and file contents.
  if (rslt != 0) {
    if (errno != ENOENT) {
      XLOG(WARN) << "error accessing file " << filePath_ << ": "
                 << folly::errnoStr(errno);
    }
    currentErrno = errno;
  }

  // If error is different, report a change. We don't want to report the
  // same error. If file does not exist (ENOENT) or is still inaccessible
  // (EACCESS) we would't want change report.
  if (statErrno_ != currentErrno) {
    return true;
  }
  // File still does not exist (no change)
  if (currentErrno == ENOENT) {
    return false;
  }
  if (currentStat.st_dev != fileStat_.st_dev ||
      currentStat.st_size != fileStat_.st_size ||
      currentStat.st_ino != fileStat_.st_ino ||
      currentStat.st_mtim.tv_sec != fileStat_.st_mtim.tv_sec ||
      currentStat.st_mtim.tv_nsec != fileStat_.st_mtim.tv_nsec) {
    return true;
  }
  return false;
}

} // namespace eden
} // namespace facebook
