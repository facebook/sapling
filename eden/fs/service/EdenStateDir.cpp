/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/service/EdenStateDir.h"

#include <folly/FileUtil.h>

using folly::StringPiece;

namespace {
constexpr StringPiece kLockFileName{"lock"};
constexpr StringPiece kTakeoverSocketName{"takeover"};
constexpr StringPiece kThriftSocketName{"socket"};
} // namespace

namespace facebook {
namespace eden {

EdenStateDir::EdenStateDir(AbsolutePathPiece path) : path_(path) {}

EdenStateDir::~EdenStateDir() {}

bool EdenStateDir::acquireLock() {
  const auto lockPath = path_ + PathComponentPiece{kLockFileName};
  auto lockFile = folly::File(lockPath.value(), O_WRONLY | O_CREAT | O_CLOEXEC);
  if (!lockFile.try_lock()) {
    return false;
  }

  takeoverLock(std::move(lockFile));
  return true;
}

void EdenStateDir::takeoverLock(folly::File lockFile) {
  writePidToLockFile(lockFile);
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
