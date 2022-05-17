/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/monitor/LogFile.h"

#include <fcntl.h>

#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>

#include "eden/fs/monitor/LogRotation.h"

namespace {
size_t getFileSize(
    const facebook::eden::AbsolutePath& path,
    const folly::File& file) noexcept {
  struct stat st;
  int rc = fstat(file.fd(), &st);
  if (rc == 0) {
    return st.st_size;
  } else {
    int errnum = errno;
    XLOG(WARN) << "unable to get file size of " << path << ": "
               << folly::errnoStr(errnum);
    // If we fail to get the file size don't bail out entirely,
    // just treat the file size as 0 for the purposes of log rotation.
    return 0;
  }
}
} // namespace

namespace facebook::eden {

LogFile::LogFile(
    const AbsolutePath& path,
    size_t maxSize,
    std::unique_ptr<LogRotationStrategy> rotationStrategy)
    : path_{path},
      log_{path.c_str(), O_CREAT | O_WRONLY | O_APPEND | O_CLOEXEC, 0644},
      logSize_{getFileSize(path_, log_)},
      maxLogSize_{maxSize},
      rotationStrategy_{std::move(rotationStrategy)},
      rotationThread_{[this] { runRotateThread(); }} {
  if (rotationStrategy_) {
    rotationStrategy_->init(path_);
  }
}

LogFile::~LogFile() {
  triggerBackgroundRotation(std::nullopt);
  rotationThread_.join();
}

int LogFile::write(const void* buffer, size_t size) {
  // Always write the full input buffer, even if it would exceed maxLogSize_.
  // This reduces the chances of us splitting the log in the middle of a message
  // (but doesn't guarantee we won't).
  auto bytesWritten = folly::writeFull(log_.fd(), buffer, size);
  if (bytesWritten == -1) {
    return errno;
  }

  // Note that our computation of state->size_ only takes into account bytes
  // that we write to the log file.  If other processes are writing to the log
  // file we don't account for this.  In general this should still be good
  // enough for our log rotation accounting purposes.  We don't expect
  // external processes to be writing lots of data to the EdenFS log file.
  logSize_ += bytesWritten;

  if (logSize_ >= maxLogSize_) {
    rotate();
  }

  return 0;
}

void LogFile::rotate() {
  // Note: we currently do not need synchronization here since edenfs_monitor
  // runs with a single main thread and performs all logging in this thread.
  // If we ever changed to logging from multiple threads we would need to add
  // synchronization here.  e.g., by putting our state in a folly::Synchronized.
  XLOG(DBG1) << "rotating log file " << path_;

  if (!rotationStrategy_) {
    return;
  }

  // Open the new log file.
  folly::File newLog;
  try {
    newLog = mainThreadRotation();
  } catch (const std::exception& ex) {
    XLOG(ERR) << "failed to rotate log file " << path_ << ": "
              << folly::exceptionStr(ex);
    // Return, and keep writing to the old log file even though it was renamed
    // to a different location now.
    return;
  }

  log_ = std::move(newLog);
  logSize_ = 0;
}

folly::File LogFile::mainThreadRotation() {
  AbsolutePath newPath;
  try {
    newPath = rotationStrategy_->renameMainLogFile();
  } catch (const std::exception& ex) {
    // If we fail to rename the file then log a warning.
    // Continue trying to re-open the log file anyway.  For instance, maybe our
    // log file was deleted out from under us, in which case the rename will
    // fail with ENOENT, but re-opening the file will re-create a new log file.
    XLOG(WARN) << "failed to rename log file " << path_
               << " for rotation: " << folly::exceptionStr(ex);
  }
  XLOG(DBG3) << "new log path " << newPath;

  // Open the new log file.
  folly::File newLog(
      path_.c_str(), O_CREAT | O_WRONLY | O_APPEND | O_CLOEXEC, 0644);

  // Trigger the background rotation thread to perform any additional
  // work to rotate/clean up/compress rotated log files.
  triggerBackgroundRotation(std::move(newPath));

  return newLog;
}

void LogFile::triggerBackgroundRotation(std::optional<AbsolutePath>&& path) {
  {
    auto queue = rotationQueue_.lock();
    queue->emplace_back(std::move(path));
  }
  rotationCV_.notify_one();
}

void LogFile::runRotateThread() {
  while (true) {
    AbsolutePath path;
    {
      auto queue = rotationQueue_.lock();
      rotationCV_.wait(queue.as_lock(), [&] { return !queue->empty(); });
      if (!queue->front().has_value()) {
        // This is the request to terminate.
        break;
      }
      path = std::move(*queue->front());
      queue->pop_front();
    }

    try {
      rotationStrategy_->performRotation(path);
    } catch (const std::exception& ex) {
      XLOG(ERR) << "error performing log rotation for " << path << ": "
                << folly::exceptionStr(ex);
      // Even if we fail on one rotation attempt, continue looping for
      // subsequent rotation requests anyway.  We don't want to abort the entire
      // program on rotation failure, nor do we want to just stop trying future
      // rotation attempts.
    }
  }
}

} // namespace facebook::eden
