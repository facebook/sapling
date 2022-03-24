/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

// https://datatracker.ietf.org/doc/rfc5531/?include_text=1

#include <vector>

#include "eden/fs/nfs/xdr/Xdr.h"

namespace facebook::eden {

enum class auth_flavor {
  AUTH_NONE = 0,
  AUTH_SYS = 1,
  AUTH_UNIX = 1, /* AUTH_UNIX is the same as AUTH_SYS */
  AUTH_SHORT = 2,
  AUTH_DH = 3,
  RPCSEC_GSS = 6
  /* and more to be defined */
};

enum class msg_type {
  CALL = 0,
  REPLY = 1,
};

enum class reply_stat { MSG_ACCEPTED = 0, MSG_DENIED = 1 };

enum class accept_stat {
  SUCCESS = 0, /* RPC executed successfully       */
  PROG_UNAVAIL = 1, /* remote hasn't exported program  */
  PROG_MISMATCH = 2, /* remote can't support version #  */
  PROC_UNAVAIL = 3, /* program can't support procedure */
  GARBAGE_ARGS = 4, /* procedure can't decode params   */
  SYSTEM_ERR = 5 /* e.g. memory allocation failure  */
};

enum class reject_stat {
  RPC_MISMATCH = 0, /* RPC version number != 2          */
  AUTH_ERROR = 1 /* remote can't authenticate caller */
};

enum class auth_stat {
  AUTH_OK = 0, /* success                        */
  /*
   * failed at remote end
   */
  AUTH_BADCRED = 1, /* bad credential (seal broken)   */
  AUTH_REJECTEDCRED = 2, /* client must begin new session  */
  AUTH_BADVERF = 3, /* bad verifier (seal broken)     */
  AUTH_REJECTEDVERF = 4, /* verifier expired or replayed   */
  AUTH_TOOWEAK = 5, /* rejected for security reasons  */
  /*
   * failed locally
   */
  AUTH_INVALIDRESP = 6, /* bogus response verifier        */
  AUTH_FAILED = 7, /* reason unknown                 */
  /*
   * AUTH_KERB errors; deprecated.  See [RFC2695]
   */
  AUTH_KERB_GENERIC = 8, /* kerberos generic error */
  AUTH_TIMEEXPIRE = 9, /* time of credential expired */
  AUTH_TKT_FILE = 10, /* problem with ticket file */
  AUTH_DECODE = 11, /* can't decode authenticator */
  AUTH_NET_ADDR = 12, /* wrong net address in ticket */
  /*
   * RPCSEC_GSS GSS related errors
   */
  RPCSEC_GSS_CREDPROBLEM = 13, /* no credentials for user */
  RPCSEC_GSS_CTXPROBLEM = 14 /* problem with context */
};

using OpaqueBytes = std::vector<uint8_t>;

struct opaque_auth {
  auth_flavor flavor;
  OpaqueBytes body;
};
EDEN_XDR_SERDE_DECL(opaque_auth, flavor, body);

constexpr uint32_t kRPCVersion = 2;

struct call_body {
  uint32_t rpcvers; /* must be equal to kRPCVersion */
  uint32_t prog;
  uint32_t vers;
  uint32_t proc;
  opaque_auth cred;
  opaque_auth verf;
  /* procedure-specific parameters start here */
};
EDEN_XDR_SERDE_DECL(call_body, rpcvers, prog, vers, proc, cred, verf);

struct rpc_msg_call {
  uint32_t xid;
  msg_type mtype; // msg_type::CALL
  call_body cbody;
};
EDEN_XDR_SERDE_DECL(rpc_msg_call, xid, mtype, cbody);

struct mismatch_info {
  uint32_t low;
  uint32_t high;
};
EDEN_XDR_SERDE_DECL(mismatch_info, low, high);

struct accepted_reply {
  opaque_auth verf;
  accept_stat stat;
};
EDEN_XDR_SERDE_DECL(accepted_reply, verf, stat);

struct rejected_reply
    : public XdrVariant<reject_stat, mismatch_info, auth_stat> {};

template <>
struct XdrTrait<rejected_reply> : public XdrTrait<rejected_reply::Base> {
  static rejected_reply deserialize(folly::io::Cursor& cursor) {
    rejected_reply ret;
    ret.tag = XdrTrait<reject_stat>::deserialize(cursor);
    switch (ret.tag) {
      case reject_stat::RPC_MISMATCH:
        ret.v = XdrTrait<mismatch_info>::deserialize(cursor);
        break;
      case reject_stat::AUTH_ERROR:
        ret.v = XdrTrait<auth_stat>::deserialize(cursor);
        break;
    }
    return ret;
  }
};

struct reply_body
    : public XdrVariant<reply_stat, accepted_reply, rejected_reply> {};

template <>
struct XdrTrait<reply_body> : public XdrTrait<reply_body::Base> {
  static reply_body deserialize(folly::io::Cursor& cursor) {
    reply_body ret;
    ret.tag = XdrTrait<reply_stat>::deserialize(cursor);
    switch (ret.tag) {
      case reply_stat::MSG_ACCEPTED:
        ret.v = XdrTrait<accepted_reply>::deserialize(cursor);
        break;
      case reply_stat::MSG_DENIED:
        ret.v = XdrTrait<rejected_reply>::deserialize(cursor);
        break;
    }
    return ret;
  }
};

struct rpc_msg_reply {
  uint32_t xid;
  msg_type mtype; // msg_type::REPLY
  reply_body rbody;
};
EDEN_XDR_SERDE_DECL(rpc_msg_reply, xid, mtype, rbody);

void serializeReply(
    folly::io::QueueAppender& ser,
    accept_stat status,
    uint32_t xid);

struct authsys_parms {
  uint32_t stamp;
  std::string machinename;
  uint32_t uid;
  uint32_t gid;
  std::vector<uint32_t> gids;
};
EDEN_XDR_SERDE_DECL(authsys_parms, stamp, machinename, uid, gid, gids);

class RpcParsingError : public std::exception {
 public:
  explicit RpcParsingError(const std::string& rpcParseFailure)
      : rpcParseFailure_(rpcParseFailure) {}

  const char* what() const noexcept override {
    return rpcParseFailure_.c_str();
  }

  const std::string& getProcedureContext() {
    return procedureContext_;
  }

  void setProcedureContext(const std::string& context) {
    procedureContext_ = context;
  }

 private:
  std::string rpcParseFailure_;
  std::string procedureContext_ = "<Unknown>";
};

} // namespace facebook::eden

#endif
