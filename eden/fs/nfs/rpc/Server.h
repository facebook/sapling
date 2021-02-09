/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

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
  virtual auth_stat checkAuthentication(const call_body& call_body);
  virtual folly::Future<folly::Unit> dispatchRpc(
      folly::io::Cursor deser,
      folly::io::Appender ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber);
};

class RpcServer {
  class RpcAcceptCallback : public folly::AsyncServerSocket::AcceptCallback {
   public:
    explicit RpcAcceptCallback(
        std::shared_ptr<RpcServerProcessor> proc,
        folly::EventBase* evb)
        : evb_(evb), proc_(proc) {}

    void connectionAccepted(
        folly::NetworkSocket fd,
        const folly::SocketAddress& clientAddr) noexcept override;

    void acceptError(const std::exception& ex) noexcept override {
      XLOG(ERR) << "acceptError: " << folly::exceptionStr(ex);
    }

   private:
    folly::EventBase* evb_;
    std::shared_ptr<RpcServerProcessor> proc_;
  };

 public:
  RpcServer(std::shared_ptr<RpcServerProcessor> proc, folly::EventBase* evb);
  ~RpcServer();

  void registerService(uint32_t progNumber, uint32_t progVersion);

  /**
   * Return the EventBase that this RpcServer is running on.
   */
  folly::EventBase* getEventBase() const {
    return evb_;
  }

  /**
   * Returns the TCP port number this RPC server is listening on.
   */
  uint16_t getPort() const;

 private:
  folly::EventBase* evb_;
  RpcAcceptCallback acceptCb_;
  std::shared_ptr<folly::AsyncServerSocket> serverSocket_;
  PortmapClient portMap_;
  std::vector<PortmapMapping> mappedPorts_;
  std::shared_ptr<RpcServerProcessor> proc_;
};

} // namespace facebook::eden

#endif
