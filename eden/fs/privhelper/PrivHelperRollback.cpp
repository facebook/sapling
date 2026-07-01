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

#include "eden/fs/utils/LinuxKernelVersion.h"
#endif

namespace facebook::eden {

#ifdef _WIN32

bool disablePrivHelperHardening() {
  return false;
}

#else

namespace {

constexpr const char* kEdenSystemConfigDir{"/etc/eden"};
#ifdef __linux__
constexpr LinuxKernelVersion kMinPrivHelperHardeningKernelVersion{5, 8};
#endif

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

#ifdef __linux__
bool isLinuxKernelTooOldForPrivHelperHardening() {
  LinuxKernelVersion version;
  try {
    version = getRunningLinuxKernelVersion();
  } catch (const std::exception& ex) {
    XLOGF(
        WARNING,
        "Cannot inspect Linux kernel version for privhelper hardening: {}",
        ex.what());
    return false;
  }

  if (version.major > kMinPrivHelperHardeningKernelVersion.major ||
      (version.major == kMinPrivHelperHardeningKernelVersion.major &&
       version.minor >= kMinPrivHelperHardeningKernelVersion.minor)) {
    return false;
  }

  XLOGF(
      WARNING,
      "Disabling privhelper hardening because Linux kernel {}.{} is older than {}.{}",
      version.major,
      version.minor,
      kMinPrivHelperHardeningKernelVersion.major,
      kMinPrivHelperHardeningKernelVersion.minor);
  return true;
}
#endif

} // namespace

bool disablePrivHelperHardening() {
#ifdef __linux__
  // The hardened mount flow uses Linux syscalls through faccessat2, which was
  // added in 5.8. Older kernels must use the legacy path-based flow.
  if (isLinuxKernelTooOldForPrivHelperHardening()) {
    return true;
  }
#endif

  // This is an emergency host-local rollback knob, so only root-controlled
  // filesystem state may disable the fd-based target checks.
  return isRootControlledPath(kEdenSystemConfigDir, S_IFDIR) &&
      isRootControlledPath(kDisablePrivHelperHardeningPath, S_IFREG);
}

#endif

} // namespace facebook::eden
