/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/portmap/Rpcbindd.h"

#include <memory>
#include <unordered_map>

#include <folly/Synchronized.h>
#include <folly/Utility.h>
#include <folly/logging/xlog.h>
#include "eden/fs/nfs/MountdRpc.h"
#include "eden/fs/nfs/portmap/RpcbindRpc.h"
#include "eden/fs/nfs/rpc/RpcServer.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {

class RpcbinddServerProcessor final : public RpcServerProcessor {
 public:
  RpcbinddServerProcessor() = default;

  RpcbinddServerProcessor(const RpcbinddServerProcessor&) = delete;
  RpcbinddServerProcessor(RpcbinddServerProcessor&&) = delete;
  RpcbinddServerProcessor& operator=(const RpcbinddServerProcessor&) = delete;
  RpcbinddServerProcessor& operator=(RpcbinddServerProcessor&&) = delete;

  ImmediateFuture<folly::Unit> dispatchRpc(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber) override;

  ImmediateFuture<folly::Unit>
  null(folly::io::Cursor deser, folly::io::QueueAppender ser, uint32_t xid);
  ImmediateFuture<folly::Unit>
  set(folly::io::Cursor deser, folly::io::QueueAppender ser, uint32_t xid);
  ImmediateFuture<folly::Unit>
  unset(folly::io::Cursor deser, folly::io::QueueAppender ser, uint32_t xid);
  ImmediateFuture<folly::Unit>
  getport(folly::io::Cursor deser, folly::io::QueueAppender ser, uint32_t xid);
  ImmediateFuture<folly::Unit>
  dump(folly::io::Cursor deser, folly::io::QueueAppender ser, uint32_t xid);
  ImmediateFuture<folly::Unit>
  callit(folly::io::Cursor deser, folly::io::QueueAppender ser, uint32_t xid);

  void recordPortNumber(uint32_t protocol, uint32_t version, uint16_t port) {
    auto lockedServers = registeredServers_.wlock();
    lockedServers->emplace(
        std::make_pair<std::pair<uint32_t, uint32_t>, uint16_t>(
            std::make_pair<uint32_t, uint32_t>(
                std::move(protocol), std::move(version)),
            std::move(port)));
  }

 private:
  using RpcProtocolNumber = uint32_t;
  using RpcProtocolVersion = uint32_t;
  using PortNumber = uint16_t;
  using RpcIdentifier = std::pair<RpcProtocolNumber, RpcProtocolVersion>;
  using RpcMappings = std::map<RpcIdentifier, PortNumber>;

  // contains the registered RPC services. Maps (server protocol number & server
  // protocol version -> port. We assume all registered services are going to
  // use TCP just because we only use TCP today. You can change that assumption,
  // but you need to add the protocol to the key.
  folly::Synchronized<RpcMappings> registeredServers_;
};

namespace {

using Handler = ImmediateFuture<folly::Unit> (RpcbinddServerProcessor::*)(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    uint32_t xid);

struct HandlerEntry {
  constexpr HandlerEntry() = default;
  constexpr HandlerEntry(folly::StringPiece n, Handler h)
      : name(n), handler(h) {}

  folly::StringPiece name;
  Handler handler = nullptr;
};

constexpr auto kRpcbindHandlers = [] {
  std::array<HandlerEntry, 6> handlers;
  handlers[folly::to_underlying(rpcbindProcs2::null)] = {
      "NULL", &RpcbinddServerProcessor::null};
  handlers[folly::to_underlying(rpcbindProcs2::set)] = {
      "SET", &RpcbinddServerProcessor::set};
  handlers[folly::to_underlying(rpcbindProcs2::unset)] = {
      "UNSET", &RpcbinddServerProcessor::unset};
  handlers[folly::to_underlying(rpcbindProcs2::getport)] = {
      "GETPORT", &RpcbinddServerProcessor::getport};
  handlers[folly::to_underlying(rpcbindProcs2::dump)] = {
      "DUMP", &RpcbinddServerProcessor::dump};
  handlers[folly::to_underlying(rpcbindProcs2::callit)] = {
      "CALLIT", &RpcbinddServerProcessor::callit};

  return handlers;
}();

} // namespace

ImmediateFuture<folly::Unit> RpcbinddServerProcessor::null(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);
  return folly::unit;
}

ImmediateFuture<folly::Unit> RpcbinddServerProcessor::set(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

ImmediateFuture<folly::Unit> RpcbinddServerProcessor::unset(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

ImmediateFuture<folly::Unit> RpcbinddServerProcessor::getport(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::SUCCESS, xid);

  auto args = XdrTrait<PortmapMapping2>::deserialize(deser);
  XLOG(DBG7) << "prog: " << args.prog;
  XLOG(DBG7) << "vers: " << args.vers;
  XLOG(DBG7) << "protocol: " << args.prot;
  if (args.prot == PortmapMapping2::kTcpProto) {
    auto lockedServers = registeredServers_.rlock();
    auto maybePort = lockedServers->find(std::make_pair<uint32_t, uint32_t>(
        std::move(args.prog), std::move(args.vers)));
    if (maybePort != lockedServers->end()) {
      XLOG(DBG7) << "port: " << maybePort->second;
      XdrTrait<uint32_t>::serialize(ser, maybePort->second);
      return folly::unit;
    }
  }
  XLOG(DBG7) << "port : none";
  XdrTrait<uint32_t>::serialize(ser, 0);
  return folly::unit;
}

ImmediateFuture<folly::Unit> RpcbinddServerProcessor::dump(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

ImmediateFuture<folly::Unit> RpcbinddServerProcessor::callit(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender ser,
    uint32_t xid) {
  serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
  return folly::unit;
}

ImmediateFuture<folly::Unit> RpcbinddServerProcessor::dispatchRpc(
    folly::io::Cursor deser,
    folly::io::QueueAppender ser,
    uint32_t xid,
    uint32_t progNumber,
    uint32_t progVersion,
    uint32_t procNumber) {
  XLOG(DBG7) << "dispatchRpc";
  if (progNumber != kPortmapProgNumber) {
    XLOG(DBG7) << "prog: " << progNumber;
    serializeReply(ser, accept_stat::PROG_UNAVAIL, xid);
    return folly::unit;
  }

  if (progVersion != kPortmapVersion2) {
    XLOG(DBG7) << "vers: " << progVersion;
    serializeReply(ser, accept_stat::PROG_MISMATCH, xid);
    XdrTrait<mismatch_info>::serialize(
        ser, mismatch_info{kPortmapVersion2, kPortmapVersion2});
    return folly::unit;
  }

  if (procNumber >= kRpcbindHandlers.size()) {
    XLOG(DBG7) << "Invalid procedure: " << procNumber;
    serializeReply(ser, accept_stat::PROC_UNAVAIL, xid);
    return folly::unit;
  }

  auto handlerEntry = kRpcbindHandlers[procNumber];

  XLOG(DBG7) << handlerEntry.name;
  return (this->*handlerEntry.handler)(std::move(deser), std::move(ser), xid);
}

Rpcbindd::Rpcbindd(
    folly::EventBase* evb,
    std::shared_ptr<folly::Executor> threadPool,
    const std::shared_ptr<StructuredLogger>& structuredLogger)
    : proc_(std::make_shared<RpcbinddServerProcessor>()),
      server_(RpcServer::create(
          proc_,
          evb,
          std::move(threadPool),
          structuredLogger)) {}

void Rpcbindd::initialize() {
  server_->initialize(folly::SocketAddress("127.0.0.1", kPortmapPortNumber));
}

void Rpcbindd::recordPortNumber(
    uint32_t protocol,
    uint32_t version,
    uint16_t port) {
  proc_->recordPortNumber(protocol, version, port);
}
} // namespace facebook::eden
