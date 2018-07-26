/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <folly/File.h>
#include <folly/FileUtil.h>
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
  return (std::chrono::steady_clock::now() - lastCheck_) < throttleDuration_;
}

bool FileChangeMonitor::invokeIfUpdated(bool noThrottle) {
  if (!noThrottle && throttle()) {
    return false;
  }

  if (!isChanged()) {
    return false;
  }

  // We want the fileChangeProcessor to be called even if open fails
  folly::File file;
  int errNum{statErrno_};
  if (statErrno_ == 0) {
    auto fileDescriptor = open(filePath_.copy().c_str(), O_RDONLY);
    if (fileDescriptor != -1) {
      file = folly::File(fileDescriptor, /**ownsFd=*/true);
      int rc = fstat(file.fd(), &fileStat_);
      if (rc != 0) {
        errNum = errno;
        statErrno_ = errNum;
        XLOG(WARN) << "error calling fstat() on " << filePath_ << ": "
                   << folly::errnoStr(errno);
      }
    } else {
      errNum = errno;
      if (errNum != ENOENT) {
        XLOG(WARN) << "error accessing file " << filePath_ << ": "
                   << folly::errnoStr(errno);
      }
    }
  }
  // We DO want to call processor since the file has changed.
  // They can determine correct action for themselves (since open failed)
  fileChangeProcessor_(std::move(file), errNum, filePath_);
  return true;
}

bool FileChangeMonitor::isChanged() {
  // Update lastCheck - we use it for throttling
  lastCheck_ = std::chrono::steady_clock::now();
  struct stat prevStat = fileStat_;
  int prevErrno = statErrno_;

  // We are using stat to check for file deltas. Since we don't open file,
  // there is no chance of TOCTOU attack.
  int rslt = stat(filePath_.c_str(), &fileStat_);
  statErrno_ = 0;
  // Log error if not ENOENT as they are unexpected and useful for debugging.
  // If error, return file change so that we update error and file contents.
  if (rslt != 0) {
    statErrno_ = errno;
    if (errno != ENOENT) {
      XLOG(WARN) << "error accessing file " << filePath_ << ": "
                 << folly::errnoStr(errno);
    }
  }

  // If error is different, report a change.
  if (statErrno_ != prevErrno) {
    return true;
  }

  // If there is a stat error, it is the same error as before. We can't check
  // the stat values. We don't really need to report a change - for example, if
  // the file is STILL does not exist (ENOENT) or is STILL inaccessible
  // (EACCESS).
  if (statErrno_ != 0) {
    return false;
  }

  if (prevStat.st_dev != fileStat_.st_dev ||
      prevStat.st_size != fileStat_.st_size ||
      prevStat.st_ino != fileStat_.st_ino ||
      prevStat.st_mode != fileStat_.st_mode ||
      prevStat.st_ctim.tv_sec != fileStat_.st_ctim.tv_sec ||
      prevStat.st_ctim.tv_nsec != fileStat_.st_ctim.tv_nsec ||
      prevStat.st_mtim.tv_sec != fileStat_.st_mtim.tv_sec ||
      prevStat.st_mtim.tv_nsec != fileStat_.st_mtim.tv_nsec) {
    return true;
  }
  return false;
}

} // namespace eden
} // namespace facebook
