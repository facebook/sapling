/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sys/types.h>
#include <cstdint>
#include <optional>
#include <string_view>
#include <vector>

#include <folly/Expected.h>

namespace facebook::eden {

/**
 * Mount source that PrivHelperServer uses for EdenFS FUSE mounts, and that
 * the scan-pins mode uses to recognize them in the mount table. The trailing
 * colon marks the mount as remote to coreutils so `df --local` skips it.
 */
inline constexpr const char kEdenFsMountSource[] = "edenfs:";

#ifdef __linux__

/**
 * A directory pinned by some process, identified by device and inode number.
 * For EdenFS FUSE mounts the inode number is the EdenFS InodeNumber.
 */
struct PinnedInode {
  uint64_t dev{};
  uint64_t ino{};

  bool operator==(const PinnedInode&) const = default;
  bool operator<(const PinnedInode& other) const {
    return std::pair{dev, ino} < std::pair{other.dev, other.ino};
  }
};

/**
 * Extract the value of the `user_id=` option from a fuse mount's option
 * string. The kernel stamps this option with the uid that created the mount,
 * so it is an authoritative record of the mount's owner.
 */
std::optional<uid_t> parseFuseUserId(std::string_view mountOptions);

/**
 * Scan procRoot for processes whose working directory or root directory
 * resolves to one of the given devices, returning the deduplicated set of
 * pinned (device, inode) pairs. Individual processes that cannot be read
 * (e.g. owned by other users when not running as root, or exited mid-scan)
 * are skipped, but failure to read procRoot itself is returned as an errno:
 * an incomplete scan must never be mistaken for an empty one.
 */
folly::Expected<std::vector<PinnedInode>, int> scanProcessPins(
    const std::vector<uint64_t>& devices,
    const char* procRoot = "/proc");

/**
 * Entry point for the `edenfs_privhelper --scan-pins` one-shot mode.
 *
 * Takes no input: the set of mounts to scan is derived entirely from the
 * mount table, restricted to fuse mounts of edenfs whose kernel-stamped
 * user_id matches the caller's real uid. This keeps the mode safe to expose
 * to arbitrary local users via the setuid binary: it parses no attacker
 * controlled input and only ever reports pins on the caller's own mounts.
 *
 * Writes one "<dev> <ino>" line per pinned inode to stdout, followed by a
 * "done" completion marker. Returns the process exit code.
 */
int runScanPinsMode();

#endif // __linux__

} // namespace facebook::eden
