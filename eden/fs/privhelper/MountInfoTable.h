/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <folly/Expected.h>

namespace facebook::eden {

#ifdef __linux__

struct MountInfo {
  uint32_t devMajor{};
  uint32_t devMinor{};
  std::string mountPoint;
  std::string mountSource;
  std::string fsType;
};

/**
 * Find mount info for an exact mount point path using
 * statmount(2)/listmount(2). Returns an error if:
 * - The syscalls are not supported (ENOSYS on older kernels)
 * - The syscalls fail for any other reason
 * Returns nullopt (success with no value) if no mount matches the path.
 */
folly::Expected<std::optional<MountInfo>, int> getMountInfoForPath(
    const char* path);

/**
 * Return all mounts whose mount point starts with the given prefix.
 * Uses listmount(2)/statmount(2). Returns an error if:
 * - The syscalls are not supported (ENOSYS on older kernels)
 * - The syscalls fail for any other reason
 */
folly::Expected<std::vector<MountInfo>, int> getMountsUnderPath(
    const std::string& prefix);

#endif

} // namespace facebook::eden
