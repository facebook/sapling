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
#include <folly/net/NetworkSocket.h>
#include "eden/fs/nfs/portmap/PortmapClient.h"
#include "eden/fs/nfs/rpc/Rpc.h"

namespace folly {
class Executor;
}

namespace facebook::eden {

class RpcServerProcessor {
 public:
  virtual ~RpcServerProcessor() = default;
  virtual auth_stat checkAuthentication(const call_body& call_body);
  virtual folly::Future<folly::Unit> dispatchRpc(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber);
};

class RpcServer {
 public:
  /**
   * Create an RPC server.
   *
   * Request will be received on the passed EventBase and dispatched to the
   * RpcServerProcessor on the passed in threadPool.
   */
  RpcServer(
      std::shared_ptr<RpcServerProcessor> proc,
      folly::EventBase* evb,
      std::shared_ptr<folly::Executor> threadPool);
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
  class RpcAcceptCallback : public folly::AsyncServerSocket::AcceptCallback,
                            public folly::DelayedDestruction {
   public:
    using UniquePtr = std::
        unique_ptr<RpcAcceptCallback, folly::DelayedDestruction::Destructor>;

    explicit RpcAcceptCallback(
        std::shared_ptr<RpcServerProcessor> proc,
        folly::EventBase* evb,
        std::shared_ptr<folly::Executor> threadPool)
        : evb_(evb),
          proc_(proc),
          threadPool_(std::move(threadPool)),
          guard_(this) {}

   private:
    void connectionAccepted(
        folly::NetworkSocket fd,
        const folly::SocketAddress& clientAddr) noexcept override;

    void acceptError(const std::exception& ex) noexcept override;

    void acceptStopped() noexcept override;

    ~RpcAcceptCallback() override = default;

    folly::EventBase* evb_;
    std::shared_ptr<RpcServerProcessor> proc_;
    std::shared_ptr<folly::Executor> threadPool_;

    /**
     * Hold a guard to ourself to avoid being deleted until the callback is
     * removed from the AsyncServerSocket.
     */
    std::optional<folly::DelayedDestruction::DestructorGuard> guard_;
  };

  folly::EventBase* evb_;
  RpcAcceptCallback::UniquePtr acceptCb_;
  folly::AsyncServerSocket::UniquePtr serverSocket_;
  std::shared_ptr<RpcServerProcessor> proc_;

  struct PortmapState {
    PortmapState() = default;

    PortmapClient portMap;
    std::vector<PortmapMapping> mappedPorts;
  };
  folly::Synchronized<std::optional<PortmapState>> portMapState_;
};

} // namespace facebook::eden

#endif
