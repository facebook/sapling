/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include "eden/common/utils/ProcMountInfo.h"

#ifdef __linux__
struct statmount;
#endif

namespace facebook::eden {

#ifdef __linux__

/**
 * Find mount info for an exact mount point path using
 * statmount(2)/listmount(2), falling back to /proc/self/mountinfo if the
 * syscalls or requested fields are unsupported, or the result exceeds the
 * syscall buffer. Returns an error if the mount table cannot be read or parsed.
 * Returns nullopt (success with no value) if no mount matches the path.
 */
folly::Expected<std::optional<MountTableEntry>, int> getMountInfoForPath(
    const char* path,
    MountInfoOptions options = {});

/**
 * Return all mounts in the current mount namespace.
 * Uses listmount(2)/statmount(2), falling back to /proc/self/mountinfo if the
 * syscalls or requested fields are unsupported, or the result exceeds the
 * syscall buffer. Returns an error if the mount table cannot be read or parsed.
 */
folly::Expected<std::vector<MountTableEntry>, int> getAllMounts(
    MountInfoOptions options = {});

/**
 * Return all mounts whose mount point starts with the given prefix.
 * Uses the same mount table and fallback as getAllMounts().
 */
folly::Expected<std::vector<MountTableEntry>, int> getMountsUnderPath(
    const std::string& prefix,
    MountInfoOptions options = {});

namespace detail {

folly::Expected<MountTableEntry, int> parseStatmount(
    const struct statmount& sm,
    MountInfoOptions options);

} // namespace detail

#endif

} // namespace facebook::eden
