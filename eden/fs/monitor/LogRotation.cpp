/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/monitor/LogRotation.h"

#include <boost/filesystem.hpp>
#include <fmt/format.h>
#include <time.h>
#include <chrono>
#include <queue>

#include <folly/Exception.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <folly/portability/SysStat.h>

#include "eden/fs/utils/Clock.h"

using folly::StringPiece;
namespace fs = boost::filesystem;

namespace facebook::eden {

LogRotationStrategy::~LogRotationStrategy() {}

TimestampLogRotation::TimestampLogRotation(
    size_t numFilesToKeep,
    std::shared_ptr<Clock> clock)
    : clock_(std::move(clock)), numFilesToKeep_{numFilesToKeep} {
  if (!clock_) {
    clock_ = std::make_shared<UnixClock>();
  }
}

TimestampLogRotation::~TimestampLogRotation() {}

void TimestampLogRotation::init(AbsolutePathPiece path) {
  path_ = path.copy();

  // Call removeOldLogFiles() immediately to clean up the log directory
  // in case it already has more than numFilesToKeep_ old files.
  try {
    removeOldLogFiles();
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error cleaning up old log files for " << path << ": "
              << folly::exceptionStr(ex);
    // Continue anyway.
    // Clean-up errors end up getting ignored during normal rotation as well,
    // since we want to proceed and still process logs rather than aborting
    // the program if we encounter errors trying to clean up old log files for
    // some reason.
  }
}

AbsolutePath TimestampLogRotation::renameMainLogFile() {
  // Compute the rotated log name
  auto newPath = computeNewPath();
  int rc = rename(path_.c_str(), newPath.c_str());
  folly::checkUnixError(rc, "rename failed");
  return newPath;
}

AbsolutePath TimestampLogRotation::computeNewPath() {
  auto timespec = clock_->getRealtime();
  struct tm ltime;
  if (!localtime_r(&timespec.tv_sec, &ltime)) {
    memset(&ltime, 0, sizeof(ltime));
  }

  // If we log multiple samples within a single second, append a numerical
  // suffix to the file name to avoid collisions.  This doesn't 100% guarantee
  // that there isn't an existing file on disk with this name, but it should be
  // unlikely.  In practice we don't usually expect to be configured such that
  // we rotate the log files many times within a single second: this normally
  // just happens during the unit tests.
  size_t suffixNum = 0;
  if (timespec.tv_sec != lastRotationTime_) {
    nextSuffix_ = 0;
    lastRotationTime_ = timespec.tv_sec;
  } else {
    suffixNum = ++nextSuffix_;
  }

  auto newName = fmt::format(
      FMT_STRING("{}-{:04d}{:02d}{:02d}.{:02}{:02d}{:02d}"),
      path_.basename().value(),
      ltime.tm_year + 1900,
      ltime.tm_mon + 1,
      ltime.tm_mday,
      ltime.tm_hour,
      ltime.tm_min,
      ltime.tm_sec);
  if (suffixNum != 0) {
    newName = fmt::format(FMT_STRING("{}.{}"), newName, suffixNum);
  }
  return path_.dirname() + PathComponentPiece(newName);
}

std::optional<TimestampLogRotation::FileSuffix>
TimestampLogRotation::parseLogSuffix(StringPiece str) {
  if (str.size() < kTimestampLength) {
    return std::nullopt;
  }
  if (str[8] != '.') {
    return std::nullopt;
  }

  auto dateNum = folly::tryTo<uint32_t>(str.subpiece(0, 8));
  if (!dateNum.hasValue()) {
    return std::nullopt;
  }
  auto timeNum = folly::tryTo<uint32_t>(str.subpiece(9, 6));
  if (!timeNum.hasValue()) {
    return std::nullopt;
  }
  if (str.size() == kTimestampLength) {
    return FileSuffix{dateNum.value(), timeNum.value(), 0};
  }

  if (str[kTimestampLength] != '.') {
    return std::nullopt;
  }
  auto suffixNum = folly::tryTo<uint32_t>(str.subpiece(kTimestampLength + 1));
  if (!suffixNum.hasValue()) {
    return std::nullopt;
  }
  return FileSuffix{dateNum.value(), timeNum.value(), suffixNum.value()};
}

std::string TimestampLogRotation::appendLogSuffix(
    StringPiece prefix,
    const FileSuffix& suffix) {
  auto finalNumber = std::get<2>(suffix);
  if (finalNumber == 0) {
    return fmt::format(
        FMT_STRING("{}{:08d}.{:06d}"),
        prefix,
        std::get<0>(suffix),
        std::get<1>(suffix));
  } else {
    return fmt::format(
        FMT_STRING("{}{:08d}.{:06d}.{}"),
        prefix,
        std::get<0>(suffix),
        std::get<1>(suffix),
        finalNumber);
  }
}

void TimestampLogRotation::performRotation(const AbsolutePath&) {
  // For now we simply prune old log files.
  // In the future perhaps we could also compress the new log file.
  removeOldLogFiles();
}

void TimestampLogRotation::removeOldLogFiles() {
  // Clean up old log files so that we have at most numFilesToKeep_ old files.
  // Keep a priority queue of the newest numFilesToKeep_ rotated file names
  std::priority_queue<
      FileSuffix,
      std::vector<FileSuffix>,
      std::greater<FileSuffix>>
      filesToKeep;

  auto prefix = path_.value() + "-";
  fs::path dirname(path_.dirname().value());
  XLOG(DBG4) << "removing old rotated log files in " << dirname;
  for (const auto& entry : fs::directory_iterator(dirname)) {
    // Only match files that start with our log file prefix
    auto entryPath = entry.path().string();
    if (!StringPiece(entryPath).startsWith(prefix)) {
      continue;
    }

    // Only match files that look like they have a valid timestamp suffix
    auto suffix =
        parseLogSuffix(StringPiece(entryPath).subpiece(prefix.size()));
    if (!suffix.has_value()) {
      continue;
    }

    XLOG(DBG9) << "log cleanup match: " << entry;
    filesToKeep.emplace(suffix.value());
    if (filesToKeep.size() > numFilesToKeep_) {
      // delete the last file.
      auto pathToRemove = appendLogSuffix(prefix, filesToKeep.top());
      filesToKeep.pop();
      XLOG(DBG5) << "remove oldest: " << pathToRemove;
      int rc = unlink(pathToRemove.c_str());
      if (rc != 0) {
        int errnum = errno;
        XLOG(WARN) << "error removing rotated log file " << pathToRemove << ": "
                   << folly::errnoStr(errnum);
        // Continue anyway.
      }
    }
  }
}

} // namespace facebook::eden
