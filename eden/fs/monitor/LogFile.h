/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <condition_variable>
#include <deque>
#include <memory>
#include <mutex>
#include <thread>

#include <folly/File.h>
#include <folly/Synchronized.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class LogRotationStrategy;

class LogFile {
 public:
  LogFile(
      const AbsolutePath& path,
      size_t maxSize,
      std::unique_ptr<LogRotationStrategy> rotationStrategy);
  ~LogFile();

  /**
   * Write data to the log file.
   *
   * If the full buffer was successfully written 0 is returned.
   * Returns an errno value on failure.
   */
  int write(const void* buffer, size_t size);

  int fd() const {
    return log_.fd();
  }

 private:
  using RotateQueue = std::deque<std::optional<AbsolutePath>>;

  void rotate();
  folly::File mainThreadRotation();
  void triggerBackgroundRotation(std::optional<AbsolutePath>&& path);
  void runRotateThread();

  AbsolutePath const path_;
  folly::File log_;
  size_t logSize_{0};
  size_t maxLogSize_{100 * 1024 * 1024};
  std::unique_ptr<LogRotationStrategy> const rotationStrategy_;

  std::condition_variable rotationCV_;
  folly::Synchronized<RotateQueue, std::mutex> rotationQueue_;
  std::thread rotationThread_;
};

} // namespace eden
} // namespace facebook
