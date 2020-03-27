/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenStateDir.h"

#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>

using folly::StringPiece;

namespace {
constexpr StringPiece kLockFileName{"lock"};
constexpr StringPiece kTakeoverSocketName{"takeover"};
constexpr StringPiece kThriftSocketName{"socket"};
} // namespace

namespace facebook {
namespace eden {

EdenStateDir::EdenStateDir(AbsolutePathPiece path)
    : path_(path), lockPath_(path + PathComponentPiece(kLockFileName)) {}

EdenStateDir::~EdenStateDir() {}

bool EdenStateDir::acquireLock() {
  auto lockFile =
      folly::File(lockPath_.value(), O_WRONLY | O_CREAT | O_CLOEXEC);
  if (!lockFile.try_lock()) {
    return false;
  }

  takeoverLock(std::move(lockFile));
  return true;
}

void EdenStateDir::takeoverLock(folly::File lockFile) {
  writePidToLockFile(lockFile);
  int rc = fstat(lockFile.fd(), &lockFileStat_);
  folly::checkUnixError(rc, "error getting lock file attributes");
  lockFile_ = std::move(lockFile);
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

  struct stat st;
  int rc = stat(lockPath_.c_str(), &st);
  if (rc != 0) {
    int errnum = errno;
    XLOG(ERR) << "EdenFS lock file no longer appears valid: "
                 "failed to stat lock file: "
              << folly::errnoStr(errnum);
    return false;
  }

  bool isSameFile =
      (st.st_dev == lockFileStat_.st_dev && st.st_ino == lockFileStat_.st_ino);
  if (!isSameFile) {
    XLOG(ERR) << "EdenFS lock file no longer appears valid: "
                 "file has been replaced";
    return false;
  }

  return true;
}

AbsolutePath EdenStateDir::getThriftSocketPath() const {
  return path_ + PathComponentPiece{kThriftSocketName};
}

AbsolutePath EdenStateDir::getTakeoverSocketPath() const {
  return path_ + PathComponentPiece{kTakeoverSocketName};
}

AbsolutePath EdenStateDir::getCheckoutStateDir(StringPiece checkoutID) const {
  return path_ + PathComponent("clients") + PathComponent(checkoutID);
}

} // namespace eden
} // namespace facebook
