/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/privhelper/PrivHelperRollback.h"

#ifndef _WIN32
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <sys/stat.h>
#include <cerrno>
#endif

namespace facebook::eden {

#ifdef _WIN32

bool disablePrivHelperHardening() {
  return false;
}

#else

namespace {

constexpr const char* kEdenSystemConfigDir{"/etc/eden"};

bool isRootControlledPath(const char* path, mode_t fileType) {
  struct stat st{};
  if (lstat(path, &st) < 0) {
    const auto err = errno;
    if (err != ENOENT) {
      XLOGF(
          WARNING,
          "Cannot inspect {} for privhelper mount check: {}",
          path,
          folly::errnoStr(err));
    }
    return false;
  }

  if ((st.st_mode & S_IFMT) != fileType || st.st_uid != 0 ||
      (st.st_mode & (S_IWGRP | S_IWOTH)) != 0) {
    XLOGF(
        WARNING,
        "Ignoring privhelper mount check at {} because it is not root-controlled",
        path);
    return false;
  }

  return true;
}

} // namespace

bool disablePrivHelperHardening() {
  // This is an emergency host-local rollback knob, so only root-controlled
  // filesystem state may disable the fd-based target checks.
  return isRootControlledPath(kEdenSystemConfigDir, S_IFDIR) &&
      isRootControlledPath(kDisablePrivHelperHardeningPath, S_IFREG);
}

#endif

} // namespace facebook::eden
