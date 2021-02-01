/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/Mountd.h"

#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <memory>
#include "eden/fs/nfs/MountdRpc.h"

namespace facebook::eden {

namespace {

class MountdServerProcessor final : public RpcServerProcessor {
 public:
  MountdServerProcessor() = default;

  MountdServerProcessor(const MountdServerProcessor&) = delete;
  MountdServerProcessor(MountdServerProcessor&&) = delete;
  MountdServerProcessor& operator=(const MountdServerProcessor&) = delete;
  MountdServerProcessor& operator=(MountdServerProcessor&&) = delete;

  folly::Future<folly::Unit> dispatchRpc(
      XdrDeSerializer deser,
      XdrSerializer ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber) override;

  folly::Future<folly::Unit>
  null(XdrDeSerializer deser, XdrSerializer ser, uint32_t xid);
  folly::Future<folly::Unit>
  mount(XdrDeSerializer deser, XdrSerializer ser, uint32_t xid);
  folly::Future<folly::Unit>
  dump(XdrDeSerializer deser, XdrSerializer ser, uint32_t xid);
  folly::Future<folly::Unit>
  umount(XdrDeSerializer deser, XdrSerializer ser, uint32_t xid);
  folly::Future<folly::Unit>
  umountAll(XdrDeSerializer deser, XdrSerializer ser, uint32_t xid);
  folly::Future<folly::Unit>
  exprt(XdrDeSerializer deser, XdrSerializer ser, uint32_t xid);
};

using Handler = folly::Future<folly::Unit> (MountdServerProcessor::*)(
    XdrDeSerializer deser,
    XdrSerializer ser,
    uint32_t xid);

struct HandlerEntry {
  constexpr HandlerEntry() = default;
  constexpr HandlerEntry(folly::StringPiece n, Handler h)
      : name(n), handler(h) {}

  folly::StringPiece name;
  Handler handler = nullptr;
};

constexpr auto kMountHandlers = [] {
  std::array<HandlerEntry, 6> handlers;
  handlers[rpc::mountProcs::null] = {"NULL", &MountdServerProcessor::null};
  handlers[rpc::mountProcs::mnt] = {"MNT", &MountdServerProcessor::mount};
  handlers[rpc::mountProcs::dump] = {"DUMP", &MountdServerProcessor::dump};
  handlers[rpc::mountProcs::umnt] = {"UMOUNT", &MountdServerProcessor::umount};
  handlers[rpc::mountProcs::umntAll] = {
      "UMOUNTALL", &MountdServerProcessor::umountAll};
  handlers[rpc::mountProcs::exprt] = {"EXPORT", &MountdServerProcessor::exprt};

  return handlers;
}();

void serializeReply(XdrSerializer& ser, rpc::accept_stat status, uint32_t xid) {
  rpc::rpc_msg_reply reply;
  reply.xid = xid;
  reply.mtype = rpc::msg_type::REPLY;

  rpc::accepted_reply accepted;
  accepted.verf.flavor = rpc::auth_flavor::AUTH_NONE;
  accepted.stat = status;

  rpc::reply_body body;
  body.set_MSG_ACCEPTED(std::move(accepted));

  reply.rbody = std::move(body);

  serializeXdr(ser, reply);
}

folly::Future<folly::Unit> MountdServerProcessor::null(
    XdrDeSerializer /*deser*/,
    XdrSerializer ser,
    uint32_t xid) {
  serializeReply(ser, rpc::accept_stat::SUCCESS, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::mount(
    XdrDeSerializer /*deser*/,
    XdrSerializer ser,
    uint32_t xid) {
  serializeReply(ser, rpc::accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::dump(
    XdrDeSerializer /*deser*/,
    XdrSerializer ser,
    uint32_t xid) {
  serializeReply(ser, rpc::accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::umount(
    XdrDeSerializer /*deser*/,
    XdrSerializer ser,
    uint32_t xid) {
  serializeReply(ser, rpc::accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::umountAll(
    XdrDeSerializer /*deser*/,
    XdrSerializer ser,
    uint32_t xid) {
  serializeReply(ser, rpc::accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::exprt(
    XdrDeSerializer /*deser*/,
    XdrSerializer ser,
    uint32_t xid) {
  serializeReply(ser, rpc::accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::dispatchRpc(
    XdrDeSerializer deser,
    XdrSerializer ser,
    uint32_t xid,
    uint32_t progNumber,
    uint32_t progVersion,
    uint32_t procNumber) {
  if (progNumber != rpc::kMountdProgNumber) {
    serializeReply(ser, rpc::accept_stat::PROG_UNAVAIL, xid);
    return folly::unit;
  }

  if (progVersion != rpc::kMountdProgVersion) {
    serializeReply(ser, rpc::accept_stat::PROG_MISMATCH, xid);
    rpc::mismatch_info mismatch = {
        rpc::kMountdProgVersion, rpc::kMountdProgVersion};
    serializeXdr(ser, mismatch);
    return folly::unit;
  }

  if (procNumber >= kMountHandlers.size()) {
    XLOG(ERR) << "Invalid procedure: " << procNumber;
    serializeReply(ser, rpc::accept_stat::PROC_UNAVAIL, xid);
    return folly::unit;
  }

  auto handlerEntry = kMountHandlers[procNumber];

  XLOG(DBG7) << handlerEntry.name;
  return (this->*handlerEntry.handler)(std::move(deser), std::move(ser), xid);
}

} // namespace

Mountd::Mountd() : server_(std::make_shared<MountdServerProcessor>()) {
  server_.registerService(rpc::kMountdProgNumber, rpc::kMountdProgVersion);
}

} // namespace facebook::eden
