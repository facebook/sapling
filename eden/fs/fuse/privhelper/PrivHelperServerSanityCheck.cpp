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

#include "eden/fs/fuse/privhelper/PrivHelperServer.h"

#include <errno.h>
#include <folly/Conv.h>
#include <folly/String.h>
#include <string>
#include "eden/fs/utils/Throw.h"

#ifndef __APPLE__
#include <sys/statfs.h>
#endif

namespace facebook::eden {

namespace {
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
    if (err == ENOTCONN) {
      // Remote filesystems like NFS, AFS, and FUSE return ENOTCONN if
      // the mount is still in the kernel mount table but the socket
      // is closed. Allow mounting in that case.
      //
      // In all likelihood, this is a mount from a prior EdenFS
      // process that crashed without unmounting.
      return;
    }
    throw_<std::domain_error>(
        "statfs failed for: ", mountPoint, ": ", folly::errnoStr(err));
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
      0x58465342 /* XFS_SB_MAGIC */,
      0x2FC12FC1 /* ZFS_SUPER_MAGIC */,
  };

  for (auto i = 0u; i < sizeof(allowedFs) / sizeof(allowedFs[0]); i++) {
    if (allowedFs[i] == fsBuf.f_type) {
      return;
    }
  }

  throw_<std::domain_error>(
      "Cannot mount over filesystem type: ", fsBuf.f_type);
#else
  (void)mountPoint;
#endif
}
} // namespace

void PrivHelperServer::sanityCheckMountPoint(const std::string& mountPoint) {
  if (getuid() == 0) {
    return;
  }

  if (access(mountPoint.c_str(), W_OK) < 0) {
    auto err = errno;
    throw_<std::domain_error>(
        "User doesn't have access to ", mountPoint, ": ", folly::errnoStr(err));
  }

  struct stat st;
  if (stat(mountPoint.c_str(), &st) < 0) {
    auto err = errno;
    throw_<std::domain_error>(
        "User doesn't have access to ", mountPoint, ": ", folly::errnoStr(err));
  }

  if (!S_ISDIR(st.st_mode)) {
    throw_<std::domain_error>(mountPoint, " isn't a directory");
  }

  if (st.st_uid != uid_) {
    throw_<std::domain_error>("User isn't the owner of: ", mountPoint);
  }

  sanityCheckFs(mountPoint);
}
} // namespace facebook::eden

#endif
