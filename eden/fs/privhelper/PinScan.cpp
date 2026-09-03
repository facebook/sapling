/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

#include "eden/fs/privhelper/PinScan.h"

#include <dirent.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>

#include <cctype>
#include <cstdio>
#include <cstring>
#include <set>

#include <folly/Conv.h>
#include <folly/String.h>

#include "eden/fs/utils/MountInfoTable.h"

namespace facebook::eden {

std::optional<uid_t> parseFuseUserId(std::string_view mountOptions) {
  std::vector<std::string_view> options;
  folly::split(',', mountOptions, options);
  for (auto option : options) {
    constexpr std::string_view kUserIdPrefix = "user_id=";
    if (option.substr(0, kUserIdPrefix.size()) == kUserIdPrefix) {
      auto value = folly::tryTo<uid_t>(option.substr(kUserIdPrefix.size()));
      if (value.hasValue()) {
        return value.value();
      }
      return std::nullopt;
    }
  }
  return std::nullopt;
}

namespace {

bool isAllDigits(const char* name) {
  if (*name == '\0') {
    return false;
  }
  for (const char* p = name; *p != '\0'; ++p) {
    if (!isdigit(static_cast<unsigned char>(*p))) {
      return false;
    }
  }
  return true;
}

} // namespace

folly::Expected<std::vector<PinnedInode>, int> scanProcessPins(
    const std::vector<uint64_t>& devices,
    const char* procRoot) {
  std::set<PinnedInode> pins;
  if (devices.empty()) {
    return std::vector<PinnedInode>{};
  }

  DIR* proc = opendir(procRoot);
  if (proc == nullptr) {
    return folly::makeUnexpected(errno);
  }

  while (true) {
    // statx below clobbers errno, so reset it before each readdir to tell
    // end-of-directory apart from a read failure.
    errno = 0;
    struct dirent* entry = readdir(proc);
    if (entry == nullptr) {
      break;
    }
    if (!isAllDigits(entry->d_name)) {
      continue;
    }
    for (const char* link : {"cwd", "root"}) {
      auto path =
          folly::to<std::string>(procRoot, "/", entry->d_name, "/", link);

      // Following a /proc magic link resolves directly to the process's
      // actual (mount, dentry) pair, so the device and inode numbers are
      // truthful regardless of the process's mount namespace or chroot.
      // AT_STATX_DONT_SYNC avoids a round trip into the target filesystem:
      // st_dev/st_ino are served from the kernel inode, so the scan does not
      // block on slow or wedged (FUSE/NFS) filesystems.
      struct statx stx{};
      if (statx(
              AT_FDCWD,
              path.c_str(),
              AT_STATX_DONT_SYNC | AT_NO_AUTOMOUNT,
              STATX_INO,
              &stx) != 0) {
        // Most commonly EACCES (another user's process when not running as
        // root) or ENOENT (the process exited mid-scan). Nothing to do but
        // skip it.
        continue;
      }

      uint64_t dev = makedev(stx.stx_dev_major, stx.stx_dev_minor);
      for (auto wanted : devices) {
        if (dev == wanted) {
          pins.insert(PinnedInode{dev, stx.stx_ino});
          break;
        }
      }
    }
  }
  const int readdirError = errno;
  closedir(proc);
  if (readdirError != 0) {
    return folly::makeUnexpected(readdirError);
  }

  return std::vector<PinnedInode>{pins.begin(), pins.end()};
}

int runScanPinsMode() {
  auto mounts = getAllMounts(
      MountInfoOptions{
          .includeMountSource = true, .includeMountOptions = true});
  if (mounts.hasError()) {
    fprintf(
        stderr,
        "scan-pins: unable to list mounts: %s\n",
        folly::errnoStr(mounts.error()).c_str());
    return 1;
  }

  const uid_t uid = getuid();
  std::vector<uint64_t> devices;
  for (const auto& mount : mounts.value()) {
    if (mount.fsType != "fuse" || mount.mountSource != kEdenFsMountSource) {
      continue;
    }
    if (parseFuseUserId(mount.mountOptions) != uid) {
      continue;
    }
    devices.push_back(makedev(mount.devMajor, mount.devMinor));
  }

  auto pins = scanProcessPins(devices);
  if (pins.hasError()) {
    fprintf(
        stderr,
        "scan-pins: unable to scan /proc: %s\n",
        folly::errnoStr(pins.error()).c_str());
    return 1;
  }
  for (const auto& pin : pins.value()) {
    printf(
        "%llu %llu\n",
        static_cast<unsigned long long>(pin.dev),
        static_cast<unsigned long long>(pin.ino));
  }
  // Completion marker: consumers must not trust output without it, since a
  // killed or crashed scan would otherwise be indistinguishable from a scan
  // that found few pins.
  printf("done\n");
  if (fflush(stdout) != 0) {
    return 1;
  }
  return 0;
}

} // namespace facebook::eden

#endif // __linux__
