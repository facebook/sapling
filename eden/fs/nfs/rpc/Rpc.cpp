/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/rpc/Rpc.h"

namespace facebook::eden::rpc {

EDEN_XDR_SERDE_IMPL(opaque_auth, flavor, body);
EDEN_XDR_SERDE_IMPL(mismatch_info, low, high);
EDEN_XDR_SERDE_IMPL(rpc_msg_call, xid, mtype, cbody);
EDEN_XDR_SERDE_IMPL(call_body, rpcvers, prog, vers, proc, cred, verf);

EDEN_XDR_VAR_SERDE_IMPL(
    rejected_reply,
    RPC_MISMATCH,
    mismatch_info,
    AUTH_ERROR,
    auth_stat)

EDEN_XDR_VAR_SERDE_IMPL(
    reply_body,
    MSG_ACCEPTED,
    accepted_reply,
    MSG_DENIED,
    rejected_reply);
EDEN_XDR_SERDE_IMPL(rpc_msg_reply, xid, mtype, rbody);

EDEN_XDR_SERDE_IMPL(accepted_reply, verf, stat);
EDEN_XDR_SERDE_IMPL(authsys_parms, stamp, machinename, uid, gid, gids);

} // namespace facebook::eden::rpc
