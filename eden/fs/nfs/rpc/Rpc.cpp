/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/rpc/Rpc.h"

namespace facebook::eden {

EDEN_XDR_SERDE_IMPL(opaque_auth, flavor, body);
EDEN_XDR_SERDE_IMPL(mismatch_info, low, high);
EDEN_XDR_SERDE_IMPL(rpc_msg_call, xid, mtype, cbody);
EDEN_XDR_SERDE_IMPL(call_body, rpcvers, prog, vers, proc, cred, verf);
EDEN_XDR_SERDE_IMPL(rpc_msg_reply, xid, mtype, rbody);
EDEN_XDR_SERDE_IMPL(accepted_reply, verf, stat);
EDEN_XDR_SERDE_IMPL(authsys_parms, stamp, machinename, uid, gid, gids);

void serializeReply(
    folly::io::QueueAppender& ser,
    accept_stat status,
    uint32_t xid) {
  rpc_msg_reply reply{
      xid,
      msg_type::REPLY,
      reply_body{{
          reply_stat::MSG_ACCEPTED,
          accepted_reply{
              opaque_auth{
                  auth_flavor::AUTH_NONE,
                  {},
              },
              status,
          },
      }},
  };
  XdrTrait<rpc_msg_reply>::serialize(ser, reply);
}

} // namespace facebook::eden

#endif
