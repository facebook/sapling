/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

// https://datatracker.ietf.org/doc/rfc5531/?include_text=1

#include <variant>
#include <vector>

#include <folly/Preprocessor.h>
#include "eden/fs/nfs/rpc/Rpc-inl.h"
#include "eden/fs/nfs/xdr/XdrDeSerializer.h"
#include "eden/fs/nfs/xdr/XdrSerializer.h"

// This is a macro that is used to emit the implementation of XDR serialization,
// deserialization and operator== for a type.
//
// The parameters the type name followed by the list of field names.
// The field names must be listed in the same order as the RPC/XDR
// definition for the type requires.  It is good practice to have that
// order match the order of the fields in the struct.
//
// Example: in the header file:
//
// struct Foo {
//    int bar;
//    int baz;
//    bool operator==(const Foo&) const;
// };
// EDEN_XDR_SERDE_DECL(Foo);
//
// Then in the cpp file:
//
// EDEN_XDR_SERDE_IMPL(Foo, bar, baz);
#define EDEN_XDR_SERDE_IMPL(STRUCT, ...)                     \
  void serializeXdr(XdrSerializer& xdr, const STRUCT& a) {   \
    FOLLY_PP_FOR_EACH(EDEN_XDR_SER, __VA_ARGS__)             \
  }                                                          \
  void deSerializeXdrInto(XdrDeSerializer& xdr, STRUCT& a) { \
    FOLLY_PP_FOR_EACH(EDEN_XDR_DE, __VA_ARGS__)              \
  }                                                          \
  bool STRUCT::operator==(const STRUCT& other) const {       \
    return FOLLY_PP_FOR_EACH(EDEN_XDR_EQ, __VA_ARGS__) 1;    \
  }

// This macro declares the XDR serializer and deserializer functions
// for a given type.
// See EDEN_XDR_SERDE_IMPL above for an example.
#define EDEN_XDR_SERDE_DECL(STRUCT)                 \
  void serializeXdr(XdrSerializer&, const STRUCT&); \
  void deSerializeXdrInto(XdrDeSerializer&, STRUCT&)

namespace facebook::eden::rpc {

enum auth_flavor {
  AUTH_NONE = 0,
  AUTH_SYS = 1,
  AUTH_SHORT = 2,
  AUTH_DH = 3,
  RPCSEC_GSS = 6
  /* and more to be defined */
};

enum msg_type {
  CALL = 0,
  REPLY = 1,
};

enum reply_stat { MSG_ACCEPTED = 0, MSG_DENIED = 1 };

enum accept_stat {
  SUCCESS = 0, /* RPC executed successfully       */
  PROG_UNAVAIL = 1, /* remote hasn't exported program  */
  PROG_MISMATCH = 2, /* remote can't support version #  */
  PROC_UNAVAIL = 3, /* program can't support procedure */
  GARBAGE_ARGS = 4, /* procedure can't decode params   */
  SYSTEM_ERR = 5 /* e.g. memory allocation failure  */
};

enum reject_stat {
  RPC_MISMATCH = 0, /* RPC version number != 2          */
  AUTH_ERROR = 1 /* remote can't authenticate caller */
};

enum auth_stat {
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

  bool operator==(const opaque_auth&) const;
};

EDEN_XDR_SERDE_DECL(opaque_auth);

constexpr uint32_t kRPCVersion = 2;

struct call_body {
  uint32_t rpcvers; /* must be equal to kRPCVersion */
  uint32_t prog;
  uint32_t vers;
  uint32_t proc;
  opaque_auth cred;
  opaque_auth verf;
  /* procedure-specific parameters start here */

  bool operator==(const call_body&) const;
};

EDEN_XDR_SERDE_DECL(call_body);

struct rpc_msg_call {
  uint32_t xid;
  msg_type mtype; // msg_type::CALL
  call_body cbody;

  bool operator==(const rpc_msg_call&) const;
};
EDEN_XDR_SERDE_DECL(rpc_msg_call);

struct mismatch_info {
  uint32_t low;
  uint32_t high;

  bool operator==(const mismatch_info&) const;
};
EDEN_XDR_SERDE_DECL(mismatch_info);

struct accepted_reply {
  opaque_auth verf;
  accept_stat stat;

  bool operator==(const accepted_reply&) const;
};
EDEN_XDR_SERDE_DECL(accepted_reply);

// This macro emits the serialization, deserialization and operator==
// implementation for an XDR tagged enum type.
//
// Usage is:
// EDEN_XDR_VAR_SERDE_IMPL(STRUCTNAME,
//    ENUM_VARIANT_1, VARIANT_TYPE_1,
//    ENUM_VARIANT_2, VARIANT_TYPE_2,
//    ...
// )
//
// When the tag portion on the wire == ENUM_VARIANT_1,
// then the data will be deserialized as VARIANT_TYPE_1
// and STRUCTNAME::tag set to the tag.
//
// When serializing, STRUCTNAME::tag is used to populate the
// wire portion of tag and variant portion is serialized.
#define EDEN_XDR_VAR_SERDE_IMPL(STRUCT, ...)                        \
  void serializeXdr(XdrSerializer& xdr, const STRUCT& v) {          \
    std::visit(                                                     \
        [&xdr](auto&& arg) {                                        \
          using T = std::decay_t<decltype(arg)>;                    \
          EDEN_XDR_FOR_EACH_PAIR(EDEN_XDR_VAR_SER, __VA_ARGS__)     \
          /* Fallthrough if the variant value cannot be matched. */ \
          throw std::runtime_error("inexhaustive variant");         \
        },                                                          \
        v.v);                                                       \
  }                                                                 \
  void deSerializeXdrInto(XdrDeSerializer& xdr, STRUCT& v) {        \
    deSerializeXdrInto(xdr, v.tag);                                 \
    switch (v.tag) {                                                \
      EDEN_XDR_FOR_EACH_PAIR(EDEN_XDR_VAR_DE, __VA_ARGS__)          \
      default:                                                      \
        throw std::runtime_error("impossible discriminant");        \
    }                                                               \
  }                                                                 \
  bool STRUCT::operator==(const STRUCT& other) const {              \
    return tag == other.tag && v == other.v;                        \
  }

// This macro is used to emit a tagged union type named STRUCT
// with a tag or discriminant type ENUM (exposed as field `tag`).
//
// The variant portion is exposed via a std::variant field named `v`,
// and accessor methods are provided.
//
// See `rejected_reply` below for a commented example.
//
// STRUCT is the name you want the struct to have.
// ENUM is the type name of the enum to use for the tag.
//
// The remaining arguments are pairs (ENUM_VARIANT, VARIANT_TYPE)
// that define the mapping from the individual enum variants of ENUM
// to the corresponding type that the variant portion should have.
//
// getter and setter accessors are generated that should help
// both preserve tag and variant consistency, as well as to avoid
// excessive variant getter type annotation from bleeding into your code.
//
// You also need to have a matching EDEN_XDR_VAR_SERDE_IMPL() call
// in an appropriate .cpp file to provide the serialization implementation.
#define EDEN_XDR_VAR_DECL(STRUCT, ENUM, ...)                        \
  struct STRUCT {                                                   \
    using tag_type = ENUM;                                          \
    ENUM tag;                                                       \
    std::variant<EDEN_XDR_VAR_TYPES(__VA_ARGS__)> v;                \
    bool operator==(const STRUCT& other) const;                     \
    EDEN_XDR_FOR_EACH_PAIR(EDEN_XDR_VAR_ACCESSOR_IMPL, __VA_ARGS__) \
  };                                                                \
  void serializeXdr(XdrSerializer& xdr, const STRUCT& v);           \
  void deSerializeXdrInto(XdrDeSerializer& xdr, STRUCT& v)

// Defines the type `rejected_reply` as:
//
// struct rejected_reply {
//   reject_stat tag;
//   std::variant<mismatch_info, auth_stat> v;
//
//   void set_RPC_MISMATCH(mismatch_info&&);
//   const mismatch_info& get_RPC_MISMATCH() const;
//   mismatch_info& get_RPC_MISMATCH();
//
//   void set_AUTH_ERROR(auth_stat&&);
//   const auth_stat& get_AUTH_ERROR() const;
//   auth_stat& get_AUTH_ERROR();
// };
EDEN_XDR_VAR_DECL(
    rejected_reply,
    reject_stat,
    RPC_MISMATCH,
    mismatch_info,
    AUTH_ERROR,
    auth_stat);

EDEN_XDR_VAR_DECL(
    reply_body,
    reply_stat,
    MSG_ACCEPTED,
    accepted_reply,
    MSG_DENIED,
    rejected_reply);

struct rpc_msg_reply {
  uint32_t xid;
  msg_type mtype; // msg_type::REPLY
  reply_body rbody;

  bool operator==(const rpc_msg_reply&) const;
};
EDEN_XDR_SERDE_DECL(rpc_msg_reply);

struct authsys_parms {
  uint32_t stamp;
  std::string machinename;
  uint32_t uid;
  uint32_t gid;
  std::vector<uint32_t> gids;
  bool operator==(const authsys_parms&) const;
};
EDEN_XDR_SERDE_DECL(authsys_parms);

} // namespace facebook::eden::rpc
