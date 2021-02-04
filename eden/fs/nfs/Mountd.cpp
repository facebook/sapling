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
      folly::io::Cursor deser,
      folly::io::Appender ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber) override;

  folly::Future<folly::Unit>
  null(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  mount(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  dump(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  umount(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  umountAll(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
  folly::Future<folly::Unit>
  exprt(folly::io::Cursor deser, folly::io::Appender ser, uint32_t xid);
};

using Handler = folly::Future<folly::Unit> (MountdServerProcessor::*)(
    folly::io::Cursor deser,
    folly::io::Appender ser,
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
  handlers[mountProcs::null] = {"NULL", &MountdServerProcessor::null};
  handlers[mountProcs::mnt] = {"MNT", &MountdServerProcessor::mount};
  handlers[mountProcs::dump] = {"DUMP", &MountdServerProcessor::dump};
  handlers[mountProcs::umnt] = {"UMOUNT", &MountdServerProcessor::umount};
  handlers[mountProcs::umntAll] = {
      "UMOUNTALL", &MountdServerProcessor::umountAll};
  handlers[mountProcs::exprt] = {"EXPORT", &MountdServerProcessor::exprt};

  return handlers;
}();

void serializeReply(
    folly::io::Appender& ser,
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

folly::Future<folly::Unit> MountdServerProcessor::null(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::mount(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::dump(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::umount(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::umountAll(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::exprt(
    folly::io::Cursor /*deser*/,
    folly::io::Appender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

folly::Future<folly::Unit> MountdServerProcessor::dispatchRpc(
    folly::io::Cursor deser,
    folly::io::Appender ser,
    uint32_t xid,
    uint32_t progNumber,
    uint32_t progVersion,
    uint32_t procNumber) {
  if (progNumber != kMountdProgNumber) {
    serializeReply(ser, accept_stat::PROG_UNAVAIL, xid);
    return folly::unit;
  }

  if (progVersion != kMountdProgVersion) {
    serializeReply(ser, accept_stat::PROG_MISMATCH, xid);
    XdrTrait<mismatch_info>::serialize(
        ser, mismatch_info{kMountdProgVersion, kMountdProgVersion});
    return folly::unit;
  }

  if (procNumber >= kMountHandlers.size()) {
    XLOG(ERR) << "Invalid procedure: " << procNumber;
    serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
    return folly::unit;
  }

  auto handlerEntry = kMountHandlers[procNumber];

  XLOG(DBG7) << handlerEntry.name;
  return (this->*handlerEntry.handler)(std::move(deser), std::move(ser), xid);
}

} // namespace

Mountd::Mountd() : server_(std::make_shared<MountdServerProcessor>()) {
  server_.registerService(kMountdProgNumber, kMountdProgVersion);
}

} // namespace facebook::eden
