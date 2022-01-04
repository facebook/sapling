/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/privhelper/NfsMountRpc.h"

namespace facebook::eden {
EDEN_XDR_SERDE_IMPL(nfstime32, seconds, nseconds);
EDEN_XDR_SERDE_IMPL(nfs_flag_set, mask_length, mask, value_length, value);
EDEN_XDR_SERDE_IMPL(nfs_fs_server_info, nfssi_currency, nfssi_info);
EDEN_XDR_SERDE_IMPL(nfs_fs_server, nfss_name, nfss_address, nfss_server_info);
EDEN_XDR_SERDE_IMPL(nfs_fs_location, nfsl_server, nfsl_rootpath);
EDEN_XDR_SERDE_IMPL(
    nfs_fs_locations_info,
    nfsli_flags,
    nfsli_valid_for,
    nfsli_root);
EDEN_XDR_SERDE_IMPL(nfs_fs_locations, nfsl_location, nfsl_locations_info);
EDEN_XDR_SERDE_IMPL(nfs_mattr, attrmask_length, attrmask, attrs);
EDEN_XDR_SERDE_IMPL(
    nfs_mount_args,
    args_version,
    args_length,
    xdr_args_version,
    nfs_mount_attrs);
} // namespace facebook::eden

#endif
