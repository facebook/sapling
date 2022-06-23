/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <folly/Try.h>
#include <sys/stat.h>
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/utils/Throw.h"

namespace facebook::eden {

/**
 * Convert the POSIX mode to NFS file type.
 */
inline ftype3 modeToFtype3(mode_t mode) {
  if (S_ISREG(mode)) {
    return ftype3::NF3REG;
  } else if (S_ISDIR(mode)) {
    return ftype3::NF3DIR;
  } else if (S_ISBLK(mode)) {
    return ftype3::NF3BLK;
  } else if (S_ISCHR(mode)) {
    return ftype3::NF3CHR;
  } else if (S_ISLNK(mode)) {
    return ftype3::NF3LNK;
  } else if (S_ISSOCK(mode)) {
    return ftype3::NF3SOCK;
  } else {
    XDCHECK(S_ISFIFO(mode));
    return ftype3::NF3FIFO;
  }
}

inline mode_t ftype3ToMode(ftype3 type) {
  switch (type) {
    case ftype3::NF3REG:
      return S_IFREG;
    case ftype3::NF3DIR:
      return S_IFDIR;
    case ftype3::NF3BLK:
      return S_IFBLK;
    case ftype3::NF3CHR:
      return S_IFCHR;
    case ftype3::NF3LNK:
      return S_IFLNK;
    case ftype3::NF3SOCK:
      return S_IFSOCK;
    case ftype3::NF3FIFO:
      return S_IFIFO;
  }
  throw_<std::domain_error>("unexpected ftype3 ", type);
}

/**
 * Convert the POSIX mode to NFS mode.
 */
inline uint32_t modeToNfsMode(mode_t mode) {
  uint32_t nfsMode = 0;

  // Owner bits:
  nfsMode |= mode & S_IRUSR ? kReadOwnerBit : 0;
  nfsMode |= mode & S_IWUSR ? kWriteOwnerBit : 0;
  nfsMode |= mode & S_IXUSR ? kExecOwnerBit : 0;

  // Group bits:
  nfsMode |= mode & S_IRGRP ? kReadGroupBit : 0;
  nfsMode |= mode & S_IWGRP ? kWriteGroupBit : 0;
  nfsMode |= mode & S_IXGRP ? kExecGroupBit : 0;

  // Other bits:
  nfsMode |= mode & S_IROTH ? kReadOtherBit : 0;
  nfsMode |= mode & S_IWOTH ? kWriteOtherBit : 0;
  nfsMode |= mode & S_IXOTH ? kExecOtherBit : 0;

  nfsMode |= mode & S_ISUID ? kSUIDBit : 0;
  nfsMode |= mode & S_ISGID ? kGIDBit : 0;

  return nfsMode;
}

/**
 * Convert a POSIX timespec to an NFS time.
 */
inline nfstime3 timespecToNfsTime(const struct timespec& time) {
  return nfstime3{
      folly::to_narrow(folly::to_unsigned(time.tv_sec)),
      folly::to_narrow(folly::to_unsigned(time.tv_nsec))};
}

/**
 * Convert a NFS time to a POSIX timespec.
 */
inline struct timespec nfsTimeToTimespec(const nfstime3& time) {
  timespec spec;
  spec.tv_sec = time.seconds;
  spec.tv_nsec = time.nseconds;
  return spec;
}

inline fattr3 statToFattr3(const struct stat& stat) {
  return fattr3{
      /*type*/ modeToFtype3(stat.st_mode),
      /*mode*/ modeToNfsMode(stat.st_mode),
      /*nlink*/ folly::to_narrow(stat.st_nlink),
      /*uid*/ stat.st_uid,
      /*gid*/ stat.st_gid,
      /*size*/ folly::to_unsigned(stat.st_size),
      /*used*/ folly::to_unsigned(stat.st_blocks) * 512u,
      /*rdev*/ specdata3{0, 0}, // TODO(xavierd)
      /*fsid*/ folly::to_unsigned(stat.st_dev),
      /*fileid*/ stat.st_ino,
#ifdef __linux__
      /*atime*/ timespecToNfsTime(stat.st_atim),
      /*mtime*/ timespecToNfsTime(stat.st_mtim),
      /*ctime*/ timespecToNfsTime(stat.st_ctim),
#else
      /*atime*/ timespecToNfsTime(stat.st_atimespec),
      /*mtime*/ timespecToNfsTime(stat.st_mtimespec),
      /*ctime*/ timespecToNfsTime(stat.st_ctimespec),
#endif
  };
}

inline pre_op_attr statToPreOpAttr(const struct stat& stat) {
  return pre_op_attr{wcc_attr{
      /*size*/ folly::to_unsigned(stat.st_size),
#ifdef __linux__
      /*mtime*/ timespecToNfsTime(stat.st_mtim),
      /*ctime*/ timespecToNfsTime(stat.st_ctim),
#else
      /*mtime*/ timespecToNfsTime(stat.st_mtimespec),
      /*ctime*/ timespecToNfsTime(stat.st_ctimespec),
#endif
  }};
}

/**
 * Convert the struct stat returned from the NfsDispatcher into a wcc_data
 * useable by NFS.
 */
inline wcc_data statToWccData(
    const std::optional<struct stat>& preStat,
    const std::optional<struct stat>& postStat) {
  return wcc_data{
      /*before*/ preStat.has_value() ? statToPreOpAttr(preStat.value())
                                     : pre_op_attr{},
      /*after*/ postStat.has_value()
          ? post_op_attr{statToFattr3(postStat.value())}
          : post_op_attr{},
  };
}

inline post_op_attr statToPostOpAttr(const folly::Try<struct stat>& stat) {
  if (stat.hasException()) {
    return post_op_attr{};
  } else {
    return post_op_attr{statToFattr3(stat.value())};
  }
}

/**
 * Determine which of the `desiredAccess`'s a client should be granted for a
 * certain file or directory based on the the `stat` of that file or directory.
 * This result is an advisory result for the access call. Clients use this call
 * to block IO that user's do not have access for, but procedures are still
 * welcome to refuse to perform an action due to access restrictions. Thus
 * this result should err on the side of being more permissive than restrictive.
 *
 * Really this should look at the uid & gid of the client issuing the request.
 * These credentials are sent as part of the RPC credentials. This gets
 * complicated because many of the authentication protocols in NFS v3 allow
 * clients to spoof their uid/gid very easily. We would need to use a
 * complicated authentication protocol like RPCSEC_GSS to be able to perform
 * proper access checks
 *
 * To simplify for now, we give user's the most permissive access they could
 * have as any user except root (we highly discourage acting as root inside an
 * EdenFS repo). This provides a little bit of access restriction, so that
 * access calls behave some what normally. However, long term we likely need to
 * implement full authentication and respond properly here. We also should
 * enforce permissions on each procedure call.
 */
uint32_t getEffectiveAccessRights(
    const struct stat& stat,
    uint32_t desiredAccess);

} // namespace facebook::eden
#endif
