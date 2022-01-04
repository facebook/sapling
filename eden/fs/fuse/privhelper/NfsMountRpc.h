/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <optional>
#include <vector>
#include "eden/fs/nfs/xdr/Xdr.h"

/*
 * XDR datastructures described in
 * https://opensource.apple.com/source/NFS/NFS-150.40.3/mount_nfs/nfs_sys_prot.x.auto.html
 */

namespace facebook::eden {

struct nfstime32 {
  int32_t seconds;
  uint32_t nseconds;
};
EDEN_XDR_SERDE_DECL(nfstime32, seconds, nseconds);

struct nfs_flag_set {
  uint32_t mask_length; /* NFS_MFLAG_BITMAP_LEN */
  uint32_t mask; /* which flags are valid */
  uint32_t value_length;
  uint32_t value; /* what each flag is set to */
};
EDEN_XDR_SERDE_DECL(nfs_flag_set, mask_length, mask, value_length, value);

enum class nfs_lock_mode {
  NFS_LOCK_MODE_ENABLED = 0,
  NFS_LOCK_MODE_DISABLED = 1,
  NFS_LOCK_MODE_LOCAL = 2
};

struct nfs_fs_server_info {
  int32_t nfssi_currency;
  bool nfssi_info; // always false
};
EDEN_XDR_SERDE_DECL(nfs_fs_server_info, nfssi_currency, nfssi_info);

struct nfs_fs_server {
  std::string nfss_name;
  std::vector<std::string> nfss_address; /* universal addresses */
  std::optional<nfs_fs_server_info> nfss_server_info;
};
EDEN_XDR_SERDE_DECL(nfs_fs_server, nfss_name, nfss_address, nfss_server_info);

// A path might be represented as its components.
using pathname = std::vector<std::string>;

struct nfs_fs_location {
  std::vector<nfs_fs_server> nfsl_server;
  pathname nfsl_rootpath;
};
EDEN_XDR_SERDE_DECL(nfs_fs_location, nfsl_server, nfsl_rootpath);

struct nfs_fs_locations_info {
  uint32_t nfsli_flags;
  int32_t nfsli_valid_for;
  pathname nfsli_root;
};
EDEN_XDR_SERDE_DECL(
    nfs_fs_locations_info,
    nfsli_flags,
    nfsli_valid_for,
    nfsli_root);

struct nfs_fs_locations {
  std::vector<nfs_fs_location> nfsl_location;
  std::optional<nfs_fs_locations_info> nfsl_locations_info;
};
EDEN_XDR_SERDE_DECL(nfs_fs_locations, nfsl_location, nfsl_locations_info);

struct nfs_mattr {
  uint32_t attrmask_length; /* NFS_MATTR_BITMAP_LEN */
  uint32_t attrmask;
  // The serialized attributes follow, can't be typed as it depends on the
  // attributes above.
  std::unique_ptr<folly::IOBuf> attrs;
};
EDEN_XDR_SERDE_DECL(nfs_mattr, attrmask_length, attrmask, attrs);

/* miscellaneous constants */
constexpr uint32_t NFS_XDRARGS_VERSION_0 = 0; /* nfs_mount_args version */
constexpr uint32_t NFS_MATTR_BITMAP_LEN =
    1; /* # XDR words in mount attributes bitmap */
constexpr uint32_t NFS_MFLAG_BITMAP_LEN =
    1; /* # XDR words in mount flags bitmap */

/*
 * Mount attributes
 *
 * Additional mount attribute notes:
 *
 * Time value attributes are specified in second.nanosecond format but
 * mount arguments may be rounded to a more appropriate unit/increment.
 *
 * The supported string values for NFS_MATTR_SOCKET_TYPE:
 *     tcp    - use TCP over IPv4 or IPv6
 *     udp    - use UDP over IPv4 or IPv6
 *     tcp6   - use TCP over IPv6 only
 *     udp6   - use UDP over IPv6 only
 *     tcp4   - use TCP over IPv4 only
 *     udp4   - use UDP over IPv4 only
 *     inet   - use TCP or UDP over IPv4 or IPv6
 *     inet4  - use TCP or UDP over IPv4 only
 *     inet6  - use TCP or UDP over IPv6 only
 */

/* mount attribute types */
using nfs_mattr_flags = nfs_flag_set;
using nfs_mattr_nfs_version = uint32_t;
using nfs_mattr_nfs_minor_version = uint32_t;
using nfs_mattr_rsize = uint32_t;
using nfs_mattr_wsize = uint32_t;
using nfs_mattr_readdirsize = uint32_t;
using nfs_mattr_readahead = uint32_t;
using nfs_mattr_acregmin = nfstime32;
using nfs_mattr_acregmax = nfstime32;
using nfs_mattr_acdirmin = nfstime32;
using nfs_mattr_acdirmax = nfstime32;
using nfs_mattr_lock_mode = nfs_lock_mode;
using nfs_mattr_security = std::vector<uint32_t>;
using nfs_mattr_maxgrouplist = uint32_t;
using nfs_mattr_socket_type = std::string;
using nfs_mattr_nfs_port = uint32_t;
using nfs_mattr_mount_port = uint32_t;
using nfs_mattr_request_timeout = nfstime32;
using nfs_mattr_soft_retry_count = uint32_t;
using nfs_mattr_dead_timeout = nfstime32;
// using	opaque nfs_mattr_fh<NFS4_FHSIZE>;
using nfs_mattr_fs_locations = nfs_fs_locations;
using nfs_mattr_mntflags = uint32_t;
using nfs_mattr_mntfrom = std::string;
using nfs_mattr_realm = std::string;
using nfs_mattr_principal = std::string;
using nfs_mattr_svcpinc = std::string;

/* mount attribute bitmap indices */
constexpr uint32_t NFS_MATTR_FLAGS = 1 << 0; /* mount flags bitmap (MFLAG_*) */
constexpr uint32_t NFS_MATTR_NFS_VERSION = 1 << 1; /* NFS protocol version */
constexpr uint32_t NFS_MATTR_NFS_MINOR_VERSION = 1
    << 2; /* NFS protocol minor version */
constexpr uint32_t NFS_MATTR_READ_SIZE = 1 << 3; /* READ RPC size */
constexpr uint32_t NFS_MATTR_WRITE_SIZE = 1 << 4; /* WRITE RPC size */
constexpr uint32_t NFS_MATTR_READDIR_SIZE = 1 << 5; /* READDIR RPC size */
constexpr uint32_t NFS_MATTR_READAHEAD = 1 << 6; /* block readahead count */
constexpr uint32_t NFS_MATTR_ATTRCACHE_REG_MIN = 1
    << 7; /* minimum attribute cache time */
constexpr uint32_t NFS_MATTR_ATTRCACHE_REG_MAX = 1
    << 8; /* maximum attribute cache time */
constexpr uint32_t NFS_MATTR_ATTRCACHE_DIR_MIN = 1
    << 9; /* minimum attribute cache time for directories */
constexpr uint32_t NFS_MATTR_ATTRCACHE_DIR_MAX = 1
    << 10; /* maximum attribute cache time for directories */
constexpr uint32_t NFS_MATTR_LOCK_MODE = 1
    << 11; /* advisory file locking mode (nfs_lock_mode) */
constexpr uint32_t NFS_MATTR_SECURITY = 1
    << 12; /* RPC security flavors to use */
constexpr uint32_t NFS_MATTR_MAX_GROUP_LIST = 1
    << 13; /* max # of RPC AUTH_SYS groups */
constexpr uint32_t NFS_MATTR_SOCKET_TYPE = 1
    << 14; /* socket transport type as a netid-like string */
constexpr uint32_t NFS_MATTR_NFS_PORT = 1
    << 15; /* port # to use for NFS protocol */
constexpr uint32_t NFS_MATTR_MOUNT_PORT = 1
    << 16; /* port # to use for MOUNT protocol */
constexpr uint32_t NFS_MATTR_REQUEST_TIMEOUT = 1
    << 17; /* initial RPC request timeout value */
constexpr uint32_t NFS_MATTR_SOFT_RETRY_COUNT = 1
    << 18; /* max RPC retransmissions for soft mounts */
constexpr uint32_t NFS_MATTR_DEAD_TIMEOUT = 1
    << 19; /* how long until unresponsive mount is considered dead */
constexpr uint32_t NFS_MATTR_FH = 1 << 20; /* file handle for mount directory */
constexpr uint32_t NFS_MATTR_FS_LOCATIONS = 1
    << 21; /* list of locations for the file system */
constexpr uint32_t NFS_MATTR_MNTFLAGS = 1 << 22; /* VFS mount flags (MNT_*) */
constexpr uint32_t NFS_MATTR_MNTFROM = 1
    << 23; /* fixed string to use for "f_mntfromname" */
constexpr uint32_t NFS_MATTR_REALM = 1
    << 24; /* Kerberos realm to use for authentication */
constexpr uint32_t NFS_MATTR_PRINCIPAL = 1
    << 25; /* Principal to use for the mount */
constexpr uint32_t NFS_MATTR_SVCPRINCIPAL = 1
    << 26; /* Kerberos principal of the server */
constexpr uint32_t NFS_MATTR_NFS_VERSION_RANGE = 1
    << 27; /* Packed version range to try */
constexpr uint32_t NFS_MATTR_KERB_ETYPE = 1
    << 28; /* Enctype to use for kerberos mounts */
constexpr uint32_t NFS_MATTR_LOCAL_NFS_PORT = 1
    << 29; /* Local transport (socket) address for NFS protocol */
constexpr uint32_t NFS_MATTR_LOCAL_MOUNT_PORT = 1
    << 30; /* Local transport (socket) address for MOUNT protocol */

/*
 * Mount flags
 */
constexpr uint32_t NFS_MFLAG_SOFT = 1
    << 0; /* soft mount (requests fail if unresponsive) */
constexpr uint32_t NFS_MFLAG_INTR = 1
    << 1; /* allow operations to be interrupted */
constexpr uint32_t NFS_MFLAG_RESVPORT = 1 << 2; /* use a reserved port */
constexpr uint32_t NFS_MFLAG_NOCONNECT = 1
    << 3; /* don't connect the socket (UDP) */
constexpr uint32_t NFS_MFLAG_DUMBTIMER = 1
    << 4; /* don't estimate RTT dynamically */
constexpr uint32_t NFS_MFLAG_CALLUMNT = 1
    << 5; /* call MOUNTPROC_UMNT on unmount */
constexpr uint32_t NFS_MFLAG_RDIRPLUS = 1
    << 6; /* request additional info when reading directories */
constexpr uint32_t NFS_MFLAG_NONEGNAMECACHE = 1
    << 7; /* don't do negative name caching */
constexpr uint32_t NFS_MFLAG_MUTEJUKEBOX = 1
    << 8; /* don't treat jukebox errors as unresponsive */
constexpr uint32_t NFS_MFLAG_EPHEMERAL = 1 << 9; /* ephemeral (mirror) mount */
constexpr uint32_t NFS_MFLAG_NOCALLBACK = 1
    << 10; /* don't provide callback RPC service */
constexpr uint32_t NFS_MFLAG_NAMEDATTR = 1
    << 11; /* don't use named attributes */
constexpr uint32_t NFS_MFLAG_NOACL = 1 << 12; /* don't support ACLs */
constexpr uint32_t NFS_MFLAG_ACLONLY = 1
    << 13; /* only support ACLs - not mode */
constexpr uint32_t NFS_MFLAG_NFC = 1 << 14; /* send NFC strings */
constexpr uint32_t NFS_MFLAG_NOQUOTA = 1
    << 15; /* don't support QUOTA requests */
constexpr uint32_t NFS_MFLAG_MNTUDP = 1
    << 16; /* MOUNT protocol should use UDP */
constexpr uint32_t NFS_MFLAG_MNTQUICK = 1
    << 17; /* use short timeouts while mounting */

/*
 * Arguments to mount an NFS file system
 *
 * Format of the buffer passed to NFS in the mount(2) system call.
 */
struct nfs_mount_args {
  uint32_t args_version; /* NFS_ARGSVERSION_XDR = 88 */
  uint32_t args_length; /* length of the entire nfs_mount_args structure */
  uint32_t xdr_args_version; /* version of nfs_mount_args structure */
  nfs_mattr nfs_mount_attrs; /* mount information */
};
EDEN_XDR_SERDE_DECL(
    nfs_mount_args,
    args_version,
    args_length,
    xdr_args_version,
    nfs_mount_attrs);

} // namespace facebook::eden

#endif
