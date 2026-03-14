/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Standalone definitions for Linux statmount(2) / listmount(2) syscalls
// introduced in kernel 6.8. Buck2's hermetic kernel-headers may not yet
// include these, so we define them locally behind #ifndef guards. At runtime,
// kernels that don't support the syscall return ENOSYS.

#pragma once

#ifdef __linux__

#include <linux/types.h>

#ifndef STATMOUNT_MNT_BASIC

/*
 * Structure for getting mount/superblock/filesystem info with statmount(2).
 *
 * The interface is similar to statx(2): individual fields or groups can be
 * selected with the @mask argument of statmount().  Kernel will set the @mask
 * field according to the supported fields.
 *
 * If string fields are selected, then the caller needs to pass a buffer that
 * has space after the fixed part of the structure.  Nul terminated strings are
 * copied there and offsets relative to @str are stored in the relevant fields.
 * If the buffer is too small, then EOVERFLOW is returned.  The actually used
 * size is returned in @size.
 */
struct statmount {
  __u32 size; /* Total size, including strings */
  __u32 mnt_opts; /* [str] Mount options of the mount */
  __u64 mask; /* What results were written */
  __u32 sb_dev_major; /* Device ID */
  __u32 sb_dev_minor;
  __u64 sb_magic; /* ..._SUPER_MAGIC */
  __u32 sb_flags; /* SB_{RDONLY,SYNCHRONOUS,DIRSYNC,LAZYTIME} */
  __u32 fs_type; /* [str] Filesystem type */
  __u64 mnt_id; /* Unique ID of mount */
  __u64 mnt_parent_id; /* Unique ID of parent (for root == mnt_id) */
  __u32 mnt_id_old; /* Reused IDs used in proc/.../mountinfo */
  __u32 mnt_parent_id_old;
  __u64 mnt_attr; /* MOUNT_ATTR_... */
  __u64 mnt_propagation; /* MS_{SHARED,SLAVE,PRIVATE,UNBINDABLE} */
  __u64 mnt_peer_group; /* ID of shared peer group */
  __u64 mnt_master; /* Mount receives propagation from this ID */
  __u64 propagate_from; /* Propagation from in current namespace */
  __u32 mnt_root; /* [str] Root of mount relative to root of fs */
  __u32 mnt_point; /* [str] Mountpoint relative to current root */
  __u64 mnt_ns_id; /* ID of the mount namespace */
  __u32 fs_subtype; /* [str] Subtype of fs_type (if any) */
  __u32 sb_source; /* [str] Source of the mount */
  __u32 opt_num; /* Number of fs options */
  __u32 opt_array; /* [str] Array of nul terminated fs options */
  __u32 opt_sec_num; /* Number of security options */
  __u32 opt_sec_array; /* [str] Array of nul terminated security options */
  __u64 __spare2[46];
  char str[]; /* Variable size part containing strings */
};

/*
 * Structure for passing mount ID and miscellaneous parameters to statmount(2)
 * and listmount(2).
 *
 * For statmount(2) @param represents the request mask.
 * For listmount(2) @param represents the last listed mount id (or zero).
 */
struct mnt_id_req {
  __u32 size;
  __u32 spare;
  __u64 mnt_id;
  __u64 param;
  __u64 mnt_ns_id;
};

/* List of all mnt_id_req versions. */
#define MNT_ID_REQ_SIZE_VER0 24 /* sizeof first published struct */

/*
 * @mask bits for statmount(2)
 */
#define STATMOUNT_SB_BASIC 0x00000001U /* Want/got sb_... */
#define STATMOUNT_MNT_BASIC 0x00000002U /* Want/got mnt_... */
#define STATMOUNT_PROPAGATE_FROM 0x00000004U /* Want/got propagate_from */
#define STATMOUNT_MNT_ROOT 0x00000008U /* Want/got mnt_root  */
#define STATMOUNT_MNT_POINT 0x00000010U /* Want/got mnt_point */
#define STATMOUNT_FS_TYPE 0x00000020U /* Want/got fs_type */
#define STATMOUNT_MNT_NS_ID 0x00000040U /* Want/got mnt_ns_id */
#define STATMOUNT_MNT_OPTS 0x00000080U /* Want/got mnt_opts */

/*
 * Special @mnt_id values that can be passed to listmount
 */
#define LSMT_ROOT 0xffffffffffffffff /* root mount */

#endif /* STATMOUNT_MNT_BASIC */

#ifndef __NR_statmount
#define __NR_statmount 457
#endif

#ifndef __NR_listmount
#define __NR_listmount 458
#endif

#endif /* __linux__ */
