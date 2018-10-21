/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>

#include "eden/fs/config/FileChangeMonitor.h"
#ifdef EDEN_WIN
#include "eden/win/fs/utils/Stub.h" // @manual
#endif

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

folly::Optional<folly::Expected<folly::File, int>>
FileChangeMonitor::checkIfUpdated(bool noThrottle) {
  folly::Optional<folly::Expected<folly::File, int>> rslt;

  if (!noThrottle && throttle()) {
    return rslt;
  }

  // Update lastCheck - we use it for throttling
  lastCheck_ = std::chrono::steady_clock::now();

  // If there was an open error last time around, we can by-pass stat because
  // the most likely scenario is for open to continue failing.
  // If there was no open error, proceed to do stat to check for file changes.
  if (!openErrno_) {
    if (!isChanged()) {
      return rslt;
    }
  }

  // Limit open/fstat calls to the following scenarios:
  // - open failed last time around. We didn't do stat (isChanged) above
  // - the file changed AND stat succeeded. We just did stat (isChanged) above
  // Even if we skip open/fstat, we will indicate the file is updated
  folly::File file;
  if (openErrno_ || !statErrno_) {
    auto fileDescriptor = open(filePath_.c_str(), O_RDONLY);
    if (fileDescriptor != -1) {
      file = folly::File(fileDescriptor, /**ownsFd=*/true);
      int rc = fstat(file.fd(), &fileStat_);
      int currentStatErrno{0};
      if (rc != 0) {
        currentStatErrno = errno;
        XLOG(WARN) << "error calling fstat() on " << filePath_ << ": "
                   << folly::errnoStr(currentStatErrno);
      }
      openErrno_ = 0;
      statErrno_ = currentStatErrno;
    } else {
      int currentOpenErrno{errno};
      // Log an error only if the error code has changed
      if (currentOpenErrno != openErrno_) {
        XLOG(WARN) << "error accessing file " << filePath_ << ": "
                   << folly::errnoStr(currentOpenErrno);
      } else {
        // Open is failing, for the same reason. It is possible that the file
        // has changed, but, not meaningful for the client.
        return rslt;
      }
      openErrno_ = currentOpenErrno;
    }
  }

  if (openErrno_ || statErrno_) {
    rslt = folly::Unexpected<int>(openErrno_ ? openErrno_ : statErrno_);
  } else {
    rslt = folly::makeExpected<int>(std::move(file));
  }
  return rslt;
}

bool FileChangeMonitor::isChanged() {
#ifndef EDEN_WIN
  struct stat currentStat;
  int prevStatErrno{statErrno_};

  // We are using stat to check for file deltas. Since we don't open file,
  // there is no chance of TOCTOU attack.
  statErrno_ = 0;
  int rslt = stat(filePath_.c_str(), &currentStat);
  if (rslt != 0) {
    statErrno_ = errno;
    // Log an error only if the error has changed (i.e., first time)
    if (statErrno_ != prevStatErrno) {
      XLOG(WARN) << "error accessing file " << filePath_ << ": "
                 << folly::errnoStr(statErrno_);
    }
  }

  // If error is different, report a change.
  if (prevStatErrno != statErrno_) {
    return true;
  }

  // If there is a stat error, we don't have a valid stat structure to check for
  // file changes. But, we now know that the stat error is the same as before
  // so, even if the file has changed, it is not interesting to the user. For
  // example, if the file STILL does not exist (ENOENT) or is STILL inaccessible
  // (EACCESS).
  if (statErrno_ != 0) {
    return false;
  }

  if (currentStat.st_dev != fileStat_.st_dev ||
      currentStat.st_size != fileStat_.st_size ||
      currentStat.st_ino != fileStat_.st_ino ||
      currentStat.st_mode != fileStat_.st_mode ||
      currentStat.st_ctim.tv_sec != fileStat_.st_ctim.tv_sec ||
      currentStat.st_ctim.tv_nsec != fileStat_.st_ctim.tv_nsec ||
      currentStat.st_mtim.tv_sec != fileStat_.st_mtim.tv_sec ||
      currentStat.st_mtim.tv_nsec != fileStat_.st_mtim.tv_nsec) {
    return true;
  }
  return false;
#else
  NOT_IMPLEMENTED();
#endif
}

} // namespace eden
} // namespace facebook
