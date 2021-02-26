/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/NfsdRpc.h"

namespace facebook::eden {
EDEN_XDR_SERDE_IMPL(specdata3, specdata1, specdata2);
EDEN_XDR_SERDE_IMPL(nfstime3, seconds, nseconds);
EDEN_XDR_SERDE_IMPL(
    fattr3,
    type,
    mode,
    nlink,
    uid,
    gid,
    size,
    used,
    rdev,
    fsid,
    fileid,
    atime,
    mtime,
    ctime);
EDEN_XDR_SERDE_IMPL(wcc_attr, size, mtime, ctime);
EDEN_XDR_SERDE_IMPL(wcc_data, before, after);
EDEN_XDR_SERDE_IMPL(sattr3, mode, uid, gid, size, atime, mtime);
EDEN_XDR_SERDE_IMPL(diropargs3, dir, name);
EDEN_XDR_SERDE_IMPL(GETATTR3args, object);
EDEN_XDR_SERDE_IMPL(GETATTR3resok, obj_attributes);
EDEN_XDR_SERDE_IMPL(LOOKUP3args, what);
EDEN_XDR_SERDE_IMPL(LOOKUP3resok, object, obj_attributes, dir_attributes);
EDEN_XDR_SERDE_IMPL(LOOKUP3resfail, dir_attributes);
EDEN_XDR_SERDE_IMPL(ACCESS3args, object, access);
EDEN_XDR_SERDE_IMPL(ACCESS3resok, obj_attributes, access);
EDEN_XDR_SERDE_IMPL(ACCESS3resfail, obj_attributes);
EDEN_XDR_SERDE_IMPL(READLINK3args, symlink);
EDEN_XDR_SERDE_IMPL(READLINK3resok, symlink_attributes, data);
EDEN_XDR_SERDE_IMPL(READLINK3resfail, symlink_attributes);
EDEN_XDR_SERDE_IMPL(CREATE3args, where, how);
EDEN_XDR_SERDE_IMPL(CREATE3resok, obj, obj_attributes, dir_wcc);
EDEN_XDR_SERDE_IMPL(CREATE3resfail, dir_wcc);
EDEN_XDR_SERDE_IMPL(MKDIR3args, where, attributes);
EDEN_XDR_SERDE_IMPL(MKDIR3resok, obj, obj_attributes, dir_wcc);
EDEN_XDR_SERDE_IMPL(MKDIR3resfail, dir_wcc);
EDEN_XDR_SERDE_IMPL(FSINFO3args, fsroot);
EDEN_XDR_SERDE_IMPL(
    FSINFO3resok,
    obj_attributes,
    rtmax,
    rtpref,
    rtmult,
    wtmax,
    wtpref,
    wtmult,
    dtpref,
    maxfilesize,
    time_delta,
    properties);
EDEN_XDR_SERDE_IMPL(FSINFO3resfail, obj_attributes);
EDEN_XDR_SERDE_IMPL(PATHCONF3args, object);
EDEN_XDR_SERDE_IMPL(
    PATHCONF3resok,
    obj_attributes,
    linkmax,
    name_max,
    no_trunc,
    chown_restricted,
    case_insensitive,
    case_preserving);
EDEN_XDR_SERDE_IMPL(PATHCONF3resfail, obj_attributes);
} // namespace facebook::eden

#endif
