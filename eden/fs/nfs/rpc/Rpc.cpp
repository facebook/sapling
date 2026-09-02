/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/rpc/Rpc.h"

#include <folly/ExceptionString.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>

namespace facebook::eden {

namespace {
// RFC 5531 bounds for AUTH_SYS credentials. Enforcing these before
// deserializing prevents a hostile length prefix from driving a huge
// allocation.
constexpr uint32_t kAuthSysMachinenameMax = 255;
constexpr uint32_t kAuthSysGidsMax = 16;
} // namespace

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

std::optional<authsys_parms> parseAuthSysCreds(const opaque_auth& auth) {
  // We only parse AUTH_SYS: EdenFS serves loopback mounts that kernel clients
  // mount with sec=sys, so AUTH_SYS (or AUTH_NONE on NULL calls) is all we
  // expect. Other flavors are treated as "no credentials". See RFC 5531 §8.2
  // for the other flavors.
  if (auth.flavor != auth_flavor::AUTH_SYS) {
    return std::nullopt;
  }

  auto buf =
      folly::IOBuf::wrapBufferAsValue(auth.body.data(), auth.body.size());
  folly::io::Cursor cursor{&buf};
  try {
    // Deserialized field by field instead of via
    // XdrTrait<authsys_parms>::deserialize so the machinename and gids
    // length prefixes can be validated first. Keep the field order in sync
    // with the authsys_parms XDR declaration.
    authsys_parms creds;
    creds.stamp = XdrTrait<uint32_t>::deserialize(cursor);

    auto nameLen = cursor.readBE<uint32_t>();
    if (nameLen > kAuthSysMachinenameMax) {
      XLOG_EVERY_MS(WARN, 60000)
          << "Malformed AUTH_SYS credential: machinename length " << nameLen;
      return std::nullopt;
    }
    creds.machinename = cursor.readFixedString(nameLen);
    if (auto padding = nameLen % 4) {
      cursor.skip(4 - padding);
    }

    creds.uid = XdrTrait<uint32_t>::deserialize(cursor);
    creds.gid = XdrTrait<uint32_t>::deserialize(cursor);

    auto gidsLen = cursor.readBE<uint32_t>();
    if (gidsLen > kAuthSysGidsMax) {
      XLOG_EVERY_MS(WARN, 60000)
          << "Malformed AUTH_SYS credential: gids length " << gidsLen;
      return std::nullopt;
    }
    creds.gids.reserve(gidsLen);
    for (uint32_t i = 0; i < gidsLen; i++) {
      creds.gids.push_back(XdrTrait<uint32_t>::deserialize(cursor));
    }
    return creds;
  } catch (const std::exception& ex) {
    XLOG_EVERY_MS(WARN, 60000)
        << "Malformed AUTH_SYS credential: " << folly::exceptionStr(ex);
    return std::nullopt;
  }
}

std::optional<RpcCallPeek> peekRpcCallHeader(const folly::IOBuf& buf) {
  if (buf.computeChainDataLength() < kMinRpcCallSize) {
    return std::nullopt;
  }
  folly::io::Cursor cursor(&buf);
  cursor.skip(4); // fragment header
  auto xid = cursor.readBE<uint32_t>();
  auto msgType = cursor.readBE<uint32_t>();
  auto rpcvers = cursor.readBE<uint32_t>();
  // TODO: prog and vers are not validated here. The fast-path will reply
  // SUCCESS (null) or PROC_UNAVAIL (unimplemented) regardless of program
  // number, which is technically an RFC 5531 violation. In practice, the
  // NFS kernel client never sends the wrong program on a dedicated socket.
  cursor.skip(8);
  auto proc = cursor.readBE<uint32_t>();

  if (msgType != static_cast<uint32_t>(msg_type::CALL) ||
      rpcvers != kRPCVersion) {
    return std::nullopt;
  }
  return RpcCallPeek{xid, proc};
}

} // namespace facebook::eden
