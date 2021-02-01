/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/SocketAddress.h>
#include <folly/io/async/AsyncServerSocket.h>
#include <folly/logging/xlog.h>
#include <folly/net/NetworkSocket.h>
#include "eden/fs/nfs/portmap/PortmapClient.h"
#include "eden/fs/nfs/rpc/Rpc.h"

namespace facebook::eden {

class RpcServerProcessor {
 public:
  virtual ~RpcServerProcessor() = default;
  virtual rpc::auth_stat checkAuthentication(const rpc::call_body& call_body);
  virtual folly::Future<folly::Unit> dispatchRpc(
      XdrDeSerializer deser,
      XdrSerializer ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber);
};

class RpcServer {
  struct RpcAcceptCallback : public folly::AsyncServerSocket::AcceptCallback {
    std::shared_ptr<RpcServerProcessor> proc;

    explicit RpcAcceptCallback(std::shared_ptr<RpcServerProcessor> proc)
        : proc(proc) {}

    void connectionAccepted(
        folly::NetworkSocket fd,
        const folly::SocketAddress& clientAddr) noexcept override;

    void acceptError(const std::exception& ex) noexcept override {
      XLOG(ERR) << "acceptError: " << folly::exceptionStr(ex);
    }
  };

 public:
  explicit RpcServer(std::shared_ptr<RpcServerProcessor> proc);
  ~RpcServer();

  void registerService(uint32_t progNumber, uint32_t progVersion);

 private:
  RpcAcceptCallback acceptCb_;
  std::shared_ptr<folly::AsyncServerSocket> serverSocket_;
  PortmapClient portMap_;
  std::vector<PortmapMapping> mappedPorts_;
  std::shared_ptr<RpcServerProcessor> proc_;
};

} // namespace facebook::eden
