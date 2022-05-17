/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <gtest/gtest_prod.h>
#include <optional>
#include <tuple>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class Clock;

/**
 * Basic API for implementing various log rotation strategies.
 *
 * Log rotation is performed in two stages:
 * - In the main thread, we first rename the existing log file to a new name,
 *   then open the log path again to create a new log file.  This should ideally
 *   be a relatively fast operation, and allow the main thread to quickly resume
 *   log forwarding.  The rest of the rotation work is then performed in a
 *   separate background thread.
 *
 * - After the main thread renames the log file, a background thread is then
 *   invoked to perform all additional rotation work.  This could involve
 *   further renaming the log file, renaming and/or deleting older log files,
 *   compressing the log file, etc.
 */
class LogRotationStrategy {
 public:
  virtual ~LogRotationStrategy();

  /**
   * init() will be called once when the LogRotationStrategy is first applied to
   * a log file.
   *
   * renameMainLogFile() will never be invoked until init() has returned.
   *
   * Implementations wish to use this method to scan the log directory and
   * perform any clean up necessary in case a previous process crashed with any
   * temporary rotation files left behind, or if the configuration has changed
   * such that some old files should be deleted.
   */
  virtual void init(AbsolutePathPiece path) = 0;

  /**
   * Rename the main log file to an alternate name.
   *
   * This will be called from the main thread.  This should be a relatively fast
   * operation, so that the main thread can resume log forwarding as soon as
   * possible.
   */
  virtual AbsolutePath renameMainLogFile() = 0;

  /**
   * Perform log rotation.
   *
   * This will be called after renameMainLogFile() with the path that was
   * returned by renameMainLogFile().  This will be called in a separate thread
   * where more expensive blocking I/O operations can be performed.
   */
  virtual void performRotation(const AbsolutePath& path) = 0;
};

/**
 * Rotate log files by appending a timestamp to each log file.
 */
class TimestampLogRotation : public LogRotationStrategy {
 public:
  explicit TimestampLogRotation(
      size_t numFilesToKeep,
      std::shared_ptr<Clock> clock = nullptr);
  ~TimestampLogRotation() override;

  void init(AbsolutePathPiece path) override;
  AbsolutePath renameMainLogFile() override;
  void performRotation(const AbsolutePath& path) override;

 private:
  FRIEND_TEST(TimestampLogRotation, parseLogSuffix);
  FRIEND_TEST(TimestampLogRotation, appendLogSuffix);
  FRIEND_TEST(TimestampLogRotation, removeOldLogFiles);

  using FileSuffix = std::tuple<uint32_t, uint32_t, uint32_t>;
  // Our timestamp suffixes consist of a 8 byte date, a period, then a 6 byte
  // time-of-day.
  static constexpr size_t kTimestampLength = 8 + 1 + 6;

  static std::optional<FileSuffix> parseLogSuffix(folly::StringPiece str);
  static std::string appendLogSuffix(
      folly::StringPiece prefix,
      const FileSuffix& suffix);
  AbsolutePath computeNewPath();
  void removeOldLogFiles();

  AbsolutePath path_;
  std::shared_ptr<Clock> clock_;
  size_t numFilesToKeep_{5};

  // In case we rotate files multiple times within the same second, we add a
  // numerical suffix to the filename.  Keep track of the last suffix we used
  // here to avoid starting over from 0 if we start removing old files from the
  // same second.  This is really only needed for unit tests which may do lots
  // of rotation in the same second.
  time_t lastRotationTime_{0};
  size_t nextSuffix_{0};
};

} // namespace facebook::eden
