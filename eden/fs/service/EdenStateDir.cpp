/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenStateDir.h"

#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>

using folly::StringPiece;

namespace facebook::eden {

namespace {
constexpr StringPiece kLockFileName{"lock"};
constexpr StringPiece kPidFileName{"pid"};
constexpr StringPiece kTakeoverSocketName{"takeover"};
constexpr StringPiece kThriftSocketName{"socket"};
constexpr PathComponentPiece kMountdSocketName{"mountd.socket"_pc};
constexpr StringPiece kHeartbeatFileNamePrefix{"heartbeat_"};
} // namespace

EdenStateDir::EdenStateDir(AbsolutePathPiece path)
    : path_(path), lockPath_(path + PathComponentPiece(kLockFileName)) {}

EdenStateDir::~EdenStateDir() = default;

std::pair<
    bool /* is_lock_acquired */,
    std::optional<std::string> /* old_daemon_pid */>
EdenStateDir::acquireLock() {
  auto lockFile =
      folly::File(lockPath_.value(), O_WRONLY | O_CREAT | O_CLOEXEC);
  try {
    if (!lockFile.try_lock()) {
      return std::make_pair(false, std::nullopt);
    }
  } catch (const std::system_error& ex) {
    throw std::runtime_error(
        fmt::format(
            "Error acquiring lock: {}. Another EdenFS process may have raced with "
            "this one. Try `eden status --wait` to check if EdenFS is starting and "
            "watch it's progress.",
            ex.what()));
  }

  return std::make_pair(true, takeoverLock(std::move(lockFile)));
}

std::optional<std::string> EdenStateDir::takeoverLock(folly::File lockFile) {
  writePidToLockFile(lockFile);
  int rc = fstat(lockFile.fd(), &lockFileStat_);
  folly::checkUnixError(rc, "error getting lock file attributes");
  lockFile_ = std::move(lockFile);

  // On Windows, also write the pid to a separate file.
  // Other processes cannot read the lock file while we are holding the lock,
  // so we store it in a separate file too.
  auto pidFilePath = path_ + PathComponentPiece(kPidFileName);

  std::string oldDaemonPid;
  std::optional<std::string> oldDaemonPidContents = std::nullopt;
  const auto pidContents = folly::to<std::string>(getpid(), "\n");
  // If the pid file already exists, we have an eden already running. We need to
  // keep its pid as old eden pid. However, the takeoverLock() function get
  // called multiple times during the startup process. We only want to keep the
  // old pid if the contents of the pid file are different from the current
  // running eden pid. Otherwise, it is the new eden pid that get into the pid
  // file before.
  if (folly::readFile(pidFilePath.c_str(), oldDaemonPid) &&
      pidContents != oldDaemonPid) {
    oldDaemonPidContents = oldDaemonPid;
  }

  folly::File pidFile(pidFilePath.c_str(), O_WRONLY | O_CREAT, 0644);
  writePidToLockFile(pidFile);
  return oldDaemonPidContents;
}

folly::File EdenStateDir::extractLock() {
  return std::move(lockFile_);
}

void EdenStateDir::writePidToLockFile(folly::File& lockFile) {
  // Write the PID (with a newline) to the lockfile.
  folly::ftruncateNoInt(lockFile.fd(), /* len */ 0);
  const auto pidContents = folly::to<std::string>(getpid(), "\n");
  folly::pwriteNoInt(lockFile.fd(), pidContents.data(), pidContents.size(), 0);
}

bool EdenStateDir::isLocked() const {
  // We only set lockFile_ once we have locked it,
  // so as long as this is set we have the lock.
  return bool(lockFile_);
}

bool EdenStateDir::isLockValid() const {
  if (!lockFile_) {
    return false;
  }

  // The st_dev and st_ino fields aren't valid on Windows, so skip the check
  // to see if the lock file is still valid.  Assume that if we acquired it
  // initially it is still valid.
  //
  // Windows generally makes it harder for users to delete or rename the
  // directory out from under an existing process while we have file handles
  // open, so this check isn't really as necessary.
#ifndef _WIN32
  struct stat st;
  int rc = stat(lockPath_.c_str(), &st);
  if (rc != 0) {
    int errnum = errno;
    XLOGF(
        ERR,
        "EdenFS lock file no longer appears valid: failed to stat lock file: {}",
        folly::errnoStr(errnum));
    return false;
  }

  bool isSameFile =
      (st.st_dev == lockFileStat_.st_dev && st.st_ino == lockFileStat_.st_ino);
  if (!isSameFile) {
    XLOG(
        ERR,
        "EdenFS lock file no longer appears valid: file has been replaced");
    return false;
  }
#endif

  return true;
}

AbsolutePath EdenStateDir::getThriftSocketPath() const {
  return path_ + PathComponentPiece{kThriftSocketName};
}

AbsolutePath EdenStateDir::getTakeoverSocketPath() const {
  return path_ + PathComponentPiece{kTakeoverSocketName};
}

AbsolutePath EdenStateDir::getMountdSocketPath() const {
  return path_ + kMountdSocketName;
}

AbsolutePath EdenStateDir::getCheckoutStateDir(StringPiece checkoutID) const {
  return path_ + PathComponent("clients") + PathComponent(checkoutID);
}

StringPiece EdenStateDir::getHeartbeatFileNamePrefix() const {
  return kHeartbeatFileNamePrefix;
}

} // namespace facebook::eden
