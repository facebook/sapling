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
#include <string>
#include <string_view>
#include <vector>

#include <folly/Expected.h>
#include <folly/container/F14Map.h>
#include <folly/container/F14Set.h>

namespace facebook::eden {

/**
 * Mount source that PrivHelperServer uses for EdenFS FUSE mounts; mount table
 * consumers recognize it through is_edenfs_mount(). The trailing colon marks
 * the mount as remote to coreutils so `df --local` skips it.
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
 * Result of a pin scan, as exchanged between `edenfs_privhelper --scan-pins`
 * and EdenFS. Its text form is one line per item:
 *
 *   dev <dev>      a device the scan covered
 *   <dev> <ino>    a pinned inode on one of those devices
 *   done           completion marker
 *
 * scannedDevices lets the consumer tell "scanned and found no pins" apart
 * from "mount not recognized by the scanner": a mount whose device is absent
 * has unknown pins and must be treated as if the scan had failed. The
 * completion marker is required because a killed or crashed scan would
 * otherwise be indistinguishable from one that found few pins.
 */
struct PinScanReport {
  folly::F14FastSet<uint64_t> scannedDevices;
  folly::F14FastMap<uint64_t, std::vector<uint64_t>> pinsByDevice;
};

std::string formatPinScanReport(const PinScanReport& report);

/**
 * Parse the text form of a report. Returns nullopt if any line is malformed
 * or the completion marker is missing.
 */
std::optional<PinScanReport> parsePinScanReport(std::string_view output);

/**
 * Entry point for the `edenfs_privhelper --scan-pins` one-shot mode.
 *
 * Takes no input: the set of mounts to scan is derived entirely from the
 * mount table, restricted to EdenFS mounts whose kernel-stamped fuse
 * user_id option matches the caller's real uid; NFS mounts carry no such
 * option, so only the caller's own FUSE mounts are scanned. This keeps the
 * mode safe to expose to arbitrary local users via the setuid binary: it
 * parses no attacker controlled input and only ever reports pins on the
 * caller's own mounts.
 *
 * Writes a PinScanReport to stdout and returns the process exit code.
 */
int runScanPinsMode();

#endif // __linux__

} // namespace facebook::eden
