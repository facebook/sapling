/*
 * @lint-ignore-every LICENSELINT
 *
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 * Copyright (C) 2001-2007  Miklos Szeredi <miklos@szeredi.hu>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/privhelper/PrivHelperServer.h"

#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <cerrno>
#include <string>

#include "eden/common/utils/ErrnoUtils.h"
#include "eden/common/utils/FSDetect.h"
#include "eden/common/utils/Throw.h"

#ifdef __APPLE__
#include <sys/mount.h>
#else
#include <sys/statfs.h>
#endif

namespace facebook::eden {

namespace {

bool getSystemMountList(std::string& out) {
#ifdef __APPLE__
  struct statfs* buf;
  int count = getmntinfo(&buf, MNT_WAIT);
  if (count == 0) {
    XLOGF(ERR, "getmntinfo failed: {}", folly::errnoStr(errno));
    return false;
  }
  for (int i = 0; i < count; i++) {
    out += fmt::format(
        "{} {} {}\n",
        buf[i].f_mntfromname,
        buf[i].f_mntonname,
        buf[i].f_fstypename);
  }
  return true;
#else
  if (folly::readFile("/proc/mounts", out)) {
    return true;
  } else {
    XLOGF(ERR, "failed to read /proc/mounts: {}", folly::errnoStr(errno));
    return false;
  }
#endif
}

/* Determines whether the given mountPoint is contained in the mount table
 * and looks like it was previously mounted by EdenFS.
 */
bool isOldEdenMount(const std::string& mountPoint) {
  std::string mounts;
  if (getSystemMountList(mounts)) {
    // TODO(T201411922): Update to std::string_view once our macOS build uses
    // C++20.
    // https://en.cppreference.com/w/cpp/string/basic_string_view/starts_with
    std::vector<folly::StringPiece> lines;
    folly::split('\n', mounts, lines);

    for (const auto& line : lines) {
      // We expect EdenFS mounts to look like the following:
      // edenfs: {mountPoint} fuse ...
      if (is_edenfs_fs_mount(line, mountPoint)) {
        return true;
      }
    }
  }
  // We couldn't verify that the mount is an old, disconnected EdenFS mount.
  // We assume it isn't to be safe.
  XLOGF(DBG4, "Could not verify that {} is an old EdenFS mount.", mountPoint);
  return false;
}

bool isErrorSafeToIgnore(int err, bool isNFS, const std::string& mountPoint) {
  // Some remote filesystems like AFS and FUSE return ENOTCONN if the mount
  // is still in the kernel mount table but the socket is closed. Allow
  // mounting in that case if the hanging mount looks like it was
  // previously mounted by EdenFS.
  //
  // Other remote filesystems (mainly NFS) return a variety of errors when
  // mounts are hanging. We've currently observed EIO and ETIMEDOUT depending
  // on whether hard or soft NFS mounts are utilized.
  //
  // In all likelihood, this is a mount from a prior EdenFS
  // process that crashed without unmounting.
  return isErrnoFromHangingMount(err, isNFS) && isOldEdenMount(mountPoint);
}

/**
 * EdenFS should only be mounted over some filesystems.
 *
 * This is copied from fusermount.c:
 * https://github.com/libfuse/libfuse/blob/master/util/fusermount.c#L990
 */
void sanityCheckFs(const std::string& mountPoint) {
#ifndef __APPLE__
  struct statfs fsBuf;
  if (statfs(mountPoint.c_str(), &fsBuf) < 0) {
    auto err = errno;
    throwf<std::domain_error>(
        "statfs failed for: {}: {}", mountPoint, folly::errnoStr(err));
  }

  constexpr typeof(fsBuf.f_type) allowedFs[] = {
      0x61756673 /* AUFS_SUPER_MAGIC */,
      0x00000187 /* AUTOFS_SUPER_MAGIC */,
      0xCA451A4E /* BCACHEFS_STATFS_MAGIC */,
      0x9123683E /* BTRFS_SUPER_MAGIC */,
      0x00C36400 /* CEPH_SUPER_MAGIC */,
      0xFF534D42 /* CIFS_MAGIC_NUMBER */,
      0x0000F15F /* ECRYPTFS_SUPER_MAGIC */,
      0X2011BAB0 /* EXFAT_SUPER_MAGIC */,
      0x0000EF53 /* EXT[234]_SUPER_MAGIC */,
      0xF2F52010 /* F2FS_SUPER_MAGIC */,
      0x65735546 /* FUSE_SUPER_MAGIC */,
      0x01161970 /* GFS2_MAGIC */,
      0x47504653 /* GPFS_SUPER_MAGIC */,
      0x0000482b /* HFSPLUS_SUPER_MAGIC */,
      0x000072B6 /* JFFS2_SUPER_MAGIC */,
      0x3153464A /* JFS_SUPER_MAGIC */,
      0x0BD00BD0 /* LL_SUPER_MAGIC */,
      0X00004D44 /* MSDOS_SUPER_MAGIC */,
      0x0000564C /* NCP_SUPER_MAGIC */,
      0x00006969 /* NFS_SUPER_MAGIC */,
      0x00003434 /* NILFS_SUPER_MAGIC */,
      0x5346544E /* NTFS_SB_MAGIC */,
      0x5346414f /* OPENAFS_SUPER_MAGIC */,
      0x794C7630 /* OVERLAYFS_SUPER_MAGIC */,
      0x52654973 /* REISERFS_SUPER_MAGIC */,
      0xFE534D42 /* SMB2_SUPER_MAGIC */,
      0x73717368 /* SQUASHFS_MAGIC */,
      0x01021994 /* TMPFS_MAGIC */,
      0x24051905 /* UBIFS_SUPER_MAGIC */,
      0x736675005346544e /* UFSD */,
      0x18031977 /* WEKA */,
      0x58465342 /* XFS_SB_MAGIC */,
      0x2FC12FC1 /* ZFS_SUPER_MAGIC */,
  };

  for (auto i = 0u; i < sizeof(allowedFs) / sizeof(allowedFs[0]); i++) {
    if (allowedFs[i] == fsBuf.f_type) {
      return;
    }
  }

  throwf<std::domain_error>(
      "Cannot mount over filesystem type: {}", fsBuf.f_type);
#else
  (void)mountPoint;
#endif
}
} // namespace

void PrivHelperServer::unmountStaleMount(const std::string& mountPoint) {
  // Attempt to unmount the stale mount.
  // Error logging is done inside unmount.
  // Always remove the mount point from mountPoints_ since it represents
  // valid mounts only.
  unmount(mountPoint.c_str(), {});
  mountPoints_.erase(mountPoint);
  XLOGF(INFO, "Successfully unmounted stale mount {}", mountPoint);
}

void PrivHelperServer::detectAndUnmountStaleMount(
    const std::string& mountPoint,
    bool isNFS,
    bool isHardMount) {
  struct stat st;
  // Stat the mount point to determine its status. If the errno matches certain
  // values, then the mount is likely hanging. We'll try to unmount it before
  // performing further sanity checks. On any other error, we throw.
  bool is_hanging = false;

  // Stat is only being used to check if the mount is hanging, not to perform
  // any sanity checks. Therefore, it should be safe to ignore this lint.
  //
  // @lint-ignore CLANGTIDY facebook-hte-BadCall-stat
  if (stat(mountPoint.c_str(), &st) < 0) {
    auto err = errno;
    XLOGF(
        WARN,
        "Error when sanity checking mount {}: {}. Checking for stale mounts.",
        mountPoint,
        folly::errnoStr(err));

    // Avoids running on hard NFS mounts since IO into hard mounts can hang
    // forever instead of returning an error.
    if (!isHardMount && isErrorSafeToIgnore(err, isNFS, mountPoint)) {
      XLOGF(
          INFO,
          "Found a stale mount {}: {}. Attempting to unmount it",
          mountPoint,
          folly::errnoStr(err));
      unmountStaleMount(mountPoint);
      is_hanging = true;
    } else {
      throwf<std::domain_error>(
          "User:{} cannot stat {}: {}",
          getuid(),
          mountPoint,
          folly::errnoStr(err));
    }
  }

  // Sometimes stat will not return this error even if the mount is
  // hanging because the stat'd path is cached by the kernel. We check for this
  // by attempting to stat a non-existent file under a non-existent folder.
  if (!isHardMount && !is_hanging) {
    // Check in case the mount point is cached in the kernel.
    XLOG(DBG4, "Double checking whether a stale mount is present.");
    std::string test_path =
        mountPoint + "/this-folder-does-not-exist/this-file-does-not-exist";
    struct stat test_st;

    // As mentioned above, using path-based stat is fine for our usescase.
    //
    // @lint-ignore CLANGTIDY facebook-hte-BadCall-stat
    if (stat(test_path.c_str(), &test_st) < 0) {
      auto err = errno;
      if (isErrnoFromHangingMount(err, isNFS)) {
        XLOGF(
            INFO,
            "Found a stale mount {}: {}. Attempting to unmount it",
            mountPoint,
            folly::errnoStr(err));
        unmountStaleMount(mountPoint);
      }
    }
    XLOGF(DBG4, "Mount {} is not stale.", mountPoint);
  }

  // On Linux/FUSE, it's possible that statfs will return an error if the mount
  // is stale, but stat won't. Try statfs as well to catch this case.
#ifdef __linux__
  struct statfs fsBuf;
  if (!isNFS && statfs(mountPoint.c_str(), &fsBuf) < 0) {
    auto err = errno;
    if (isErrorSafeToIgnore(err, isNFS, mountPoint)) {
      XLOGF(
          INFO,
          "Found a stale mount {}: {}. Attempting to unmount it",
          mountPoint,
          folly::errnoStr(err));
      unmountStaleMount(mountPoint);
    } else {
      throwf<std::domain_error>(
          "statfs failed for: {}: {}", mountPoint, folly::errnoStr(err));
    }
  }
#endif
}

void PrivHelperServer::sanityCheckMountPoint(
    const std::string& mountPoint,
    bool isNFS,
    bool isHardMount) {
  XLOGF(INFO, "Sanity checking mount {}", mountPoint);
  if (getuid() == 0) {
    XLOG(INFO, "Skipping sanity check for root user.");
    return;
  }

  detectAndUnmountStaleMount(mountPoint, isNFS, isHardMount);

  if (access(mountPoint.c_str(), W_OK) < 0) {
    auto err = errno;
    throwf<std::domain_error>(
        "User:{} doesn't have write access to {}: {}",
        getuid(),
        mountPoint,
        folly::errnoStr(err));
  }

  // At this point, any stat errors are not due to a stale mount.
  struct stat st {};
  auto fd = open(mountPoint.c_str(), O_RDONLY);
  if (fd == -1 || fstat(fd, &st) < 0) {
    auto err = errno;
    throwf<std::domain_error>(
        "User:{} cannot stat {}: {}",
        getuid(),
        mountPoint,
        folly::errnoStr(err));
  }

  if (!S_ISDIR(st.st_mode)) {
    throwf<std::domain_error>("{} isn't a directory", mountPoint);
  }

  if (st.st_uid != uid_) {
    throwf<std::domain_error>(
        "User:{} isn't the owner of: {}", uid_, mountPoint);
  }

  sanityCheckFs(mountPoint);
}
} // namespace facebook::eden

#endif
