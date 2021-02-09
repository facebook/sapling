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
