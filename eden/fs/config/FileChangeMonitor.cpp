/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>

#include "eden/fs/config/FileChangeMonitor.h"
#include "eden/fs/utils/StatTimes.h"
#include "eden/fs/utils/TimeUtil.h"

namespace facebook::eden {

FileChangeReason hasFileChanged(
    const struct stat& stat1,
    const struct stat& stat2) noexcept {
  if (stat1.st_size != stat2.st_size) {
    return FileChangeReason::SIZE;
  }
  if (stMtime(stat1) != stMtime(stat2)) {
    return FileChangeReason::MTIME;
  }
  if (!folly::kIsWindows) {
    // On Windows, these stat entries are synthesized by MSVCRT, and
    // checking them may falsely consider files changed.
    if (stat1.st_dev != stat2.st_dev) {
      return FileChangeReason::DEV;
    }
    if (stat1.st_ino != stat2.st_ino) {
      return FileChangeReason::INO;
    }
    if (stat1.st_mode != stat2.st_mode) {
      return FileChangeReason::MODE;
    }
    if (stCtime(stat1) != stCtime(stat2)) {
      return FileChangeReason::CTIME_;
    }
  }
  return FileChangeReason::NONE;
}

std::string_view FileChangeReason::str() const {
  switch (reason) {
    case NONE:
      return "none";
    case SIZE:
      return "size";
    case DEV:
      return "dev";
    case INO:
      return "ino";
    case MODE:
      return "mode";
    case CTIME_:
      return "ctime";
    case MTIME:
      return "mtime";
  }
  return "invalid reason value";
}

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

std::optional<folly::Expected<folly::File, int>>
FileChangeMonitor::checkIfUpdated(bool noThrottle) {
  std::optional<folly::Expected<folly::File, int>> rslt;

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
  struct stat currentStat;
  int prevStatErrno{statErrno_};

  // We are using stat to check for file deltas. Since we don't open file,
  // there is no chance of TOCTOU attack.
  statErrno_ = 0;
  int rslt = stat(filePath_.c_str(), &currentStat);
  if (rslt != 0) {
    statErrno_ = errno;
    // Log unexpected errors accessing the file (e.g., permission denied, or
    // unexpected file type).  Don't log if the file simply doesn't exist.
    // Also only log when the error changes, so that we don't repeatedly log
    // the same message.
    if (statErrno_ != ENOENT && statErrno_ != prevStatErrno) {
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

  return hasFileChanged(currentStat, fileStat_) ? true : false;
}

} // namespace facebook::eden
