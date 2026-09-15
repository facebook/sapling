/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Utility.h>
#include <folly/io/IOBuf.h>
#include <folly/io/IOBufQueue.h>
#include <folly/lang/Bits.h>

#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"

namespace facebook::eden {

/**
 * Build an AUTH_SYS credential carrying the given parameters.
 */
opaque_auth makeAuthSysCred(const authsys_parms& creds);

/**
 * Serialize an NFSv3 request for the given procedure with the given
 * credential, framed with a record-mark fragment header. serializeArgs is
 * called with the QueueAppender to append the procedure arguments.
 */
template <typename SerializeArgs>
std::unique_ptr<folly::IOBuf> buildNfsRequestImpl(
    uint32_t xid,
    nfsv3Procs proc,
    opaque_auth cred,
    SerializeArgs&& serializeArgs) {
  folly::IOBufQueue queue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender ser(&queue, 1024);

  XdrTrait<uint32_t>::serialize(ser, 0); // fragment header placeholder
  rpc_msg_call call{
      xid,
      msg_type::CALL,
      call_body{
          kRPCVersion,
          kNfsdProgNumber,
          kNfsd3ProgVersion,
          folly::to_underlying(proc),
          std::move(cred),
          opaque_auth{auth_flavor::AUTH_NONE, {}},
      },
  };
  XdrTrait<rpc_msg_call>::serialize(ser, call);
  serializeArgs(ser);

  auto len = static_cast<uint32_t>(queue.chainLength() - sizeof(uint32_t));
  auto buf = queue.move();
  auto* header = reinterpret_cast<uint32_t*>(buf->writableData());
  *header = folly::Endian::big(len | 0x80000000);
  return buf;
}

template <typename Args>
std::unique_ptr<folly::IOBuf> buildNfsRequest(
    uint32_t xid,
    nfsv3Procs proc,
    opaque_auth cred,
    const Args& args) {
  return buildNfsRequestImpl(
      xid, proc, std::move(cred), [&](folly::io::QueueAppender& ser) {
        XdrTrait<Args>::serialize(ser, args);
      });
}

inline std::unique_ptr<folly::IOBuf>
buildNfsRequest(uint32_t xid, nfsv3Procs proc, opaque_auth cred) {
  return buildNfsRequestImpl(
      xid, proc, std::move(cred), [](folly::io::QueueAppender&) {});
}

} // namespace facebook::eden
