/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <vector>

#include <folly/SocketAddress.h>
#include <folly/io/async/AsyncServerSocket.h>
#include <folly/io/async/AsyncSocket.h>
#include <folly/net/NetworkSocket.h>

#include "eden/fs/inodes/FsChannel.h"
#include "eden/fs/nfs/portmap/PortmapClient.h"
#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/utils/EventBaseState.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace folly {
class Executor;
}

namespace facebook::eden {

class StructuredLogger;

enum class RpcStopReason {
  /**
   * The socket was closed. For nfsd3, the socket closing means the
   * mount point was unmounted (perhaps by EdenFS). For mountd, this
   * means a normal connection was closed, but that case is typical.
   */
  UNMOUNT,
  /**
   * Reading from the socket failed. There's nothing else to do, so
   * the server stopped.
   */
  ERROR,
  /**
   * takeoverStop() was called.
   */
  TAKEOVER,
};

struct RpcStopData final : FsStopData {
  bool isUnmounted() override;
  FsChannelInfo extractTakeoverInfo() override;

  /**
   * The reason why the connection was stopped.
   *
   * If multiple events occurred that triggered shutdown, only one will be
   * reported here.
   */
  RpcStopReason reason;

  /**
   * The socket for communicating with the kernel, if it is still valid
   * to use.
   */
  folly::File socketToKernel;
};

class RpcServerProcessor {
 public:
  virtual ~RpcServerProcessor() = default;
  virtual auth_stat checkAuthentication(const call_body& call_body);
  virtual ImmediateFuture<folly::Unit> dispatchRpc(
      folly::io::Cursor deser,
      folly::io::QueueAppender ser,
      uint32_t xid,
      uint32_t progNumber,
      uint32_t progVersion,
      uint32_t procNumber);
  virtual void clientConnected();
  virtual void onShutdown(RpcStopData stopData);
};

class RpcServer;

/**
 * RpcConnectionHandler manages connected RPC sockets, whether for NFS or Mountd
 * or Rpcbind.
 *
 * Right now, it only supports stream sockets, such as TCP. If we add support
 * for UDP or unix datagram sockets, rename this to RpcStreamHandler and
 * introduce an RpcDatagramHandler.
 */
class RpcConnectionHandler : public folly::DelayedDestruction,
                             private folly::AsyncReader::ReadCallback,
                             private folly::AsyncWriter::WriteCallback {
 public:
  using UniquePtr = std::
      unique_ptr<RpcConnectionHandler, folly::DelayedDestruction::Destructor>;

  /**
   * Build a RpcConnectionHandler.
   *
   * When the returned UniquePtr is dropped, this class will stay alive until
   * the client drops the connection, at which time the memory will be released
   * and the socket will be closed.
   */
  template <class... Args>
  static UniquePtr create(Args&&... args) {
    return UniquePtr(
        new RpcConnectionHandler(std::forward<Args>(args)...),
        folly::DelayedDestruction::Destructor());
  }

  /**
   * Unregister the reader, so that no more data is read from the socket.
   *
   * This must be called on the main event base of the socket. This is because
   * we are going to access state_ which can only be accessed on the main
   * eventbase and we do operations on the socket (which generally can only be
   * done on the main eventbase).
   */
  folly::SemiFuture<folly::Unit> takeoverStop();

 private:
  RpcConnectionHandler(
      std::shared_ptr<RpcServerProcessor> proc,
      folly::AsyncSocket::UniquePtr&& socket,
      std::shared_ptr<folly::Executor> threadPool,
      const std::shared_ptr<StructuredLogger>& structuredLogger,
      std::weak_ptr<RpcServer> owningServer);

  // AsyncReader::ReadCallback

  void getReadBuffer(void** bufP, size_t* lenP) override;

  void readDataAvailable(size_t len) noexcept override;

  bool isBufferMovable() noexcept override;

  void readBufferAvailable(
      std::unique_ptr<folly::IOBuf> readBuf) noexcept override;

  void readEOF() noexcept override;

  void readErr(const folly::AsyncSocketException& ex) noexcept override;

  // AsyncWriter::WriteCallback

  void writeSuccess() noexcept override {}

  void writeErr(
      size_t /*bytesWritten*/,
      const folly::AsyncSocketException& ex) noexcept override;

  /**
   * Parse the buffer that was just read from the socket. Complete RPC buffers
   * will be dispatched to the RpcServerProcessor.
   */
  void tryConsumeReadBuffer() noexcept;

  /**
   * Delete the reader, called when the socket is closed or on takeover.
   *
   * This must be called on the main event base of the socket. This is
   * because we are going to access state_ which can only be accessed on the
   * main eventbase and we do operations on the socket (which generally can
   * only be done on the main eventbase).
   */
  folly::SemiFuture<folly::Unit> resetReader(RpcStopReason stopReason);

  /**
   * Try to read one request from the buffer.
   *
   * Return a nullptr if no complete RPC request can be read.
   */
  std::unique_ptr<folly::IOBuf> readOneRequest() noexcept;

  /**
   * Dispatch the RPC request contained in the input buffer to the
   * RpcServerProcessor.
   */
  void dispatchAndReply(
      std::unique_ptr<folly::IOBuf> input,
      DestructorGuard guard);

  /**
   * Reply to an rpc call with an error.
   * This function assumes that some data may have already been written to the
   * output buffer. That output is discarded and we reply with an error message
   * instead.
   * This should be used if an exception is thrown while disptaching a request
   * and the exception bubbles up to the rpc server.
   */
  void replyServerError(
      accept_stat err,
      uint32_t xid,
      std::unique_ptr<folly::IOBufQueue>& outputBuffer);

  /**
   * Locally logs an error due to parsing an NFS request as well as log
   * externally, so we can monitor these errors.
   */
  void recordParsingError(
      RpcParsingError& err,
      std::unique_ptr<folly::IOBuf> input);

  /**
   * Processor to handle the requests.
   */
  std::shared_ptr<RpcServerProcessor> proc_;

  /**
   * Socket we are listening on.
   */
  folly::AsyncSocket::UniquePtr sock_;

  /**
   * NFS requests will be dispatched to this Executor. This is done to avoid
   * blocking the event base that is reading on the socket.
   */
  std::shared_ptr<folly::Executor> threadPool_;

  /**
   * This is a logger for error events. Inside a Meta environment, these events
   * are exported off the machine this EdenFS instace is running on. This is
   * where you log anomalous things that you want to monitor accross the fleet.
   */
  std::shared_ptr<StructuredLogger> errorLogger_;

  folly::IOBufQueue readBuf_{folly::IOBufQueue::cacheChainLength()};

  /**
   * Status for the rpc connection. The State may only be accessed from the
   * socket's eventbase thread. We use this invariant so that we don't have to
   * add a lock to the state which would be used in the hot path for every
   * request.
   */
  struct State {
    // If set, shutdown has started.
    std::optional<RpcStopReason> stopReason;

    // number of requests we are in the middle of processing
    size_t pendingRequests = 0;
  };

  EventBaseState<State> state_;

  /**
   * Promise that we set during shutdown when we finish processing all the
   * pending requests.
   *
   * pendingRequestsComplete_ is fulfilled the first time
   * pendingRequests == 0 and stopReason.has_value().
   */
  folly::Promise<folly::Unit> pendingRequestsComplete_;

  /**
   * RpcServer that initiated this RpcConnectionHandler. We keep a reference to
   * this so that we can notify the server when we are shutting down. The server
   * should outlive all of it's connections, but if the server has already been
   * shutdown then we can just skip notifying it that we are shutting down.
   */
  std::weak_ptr<RpcServer> owningServer_;
};

class RpcServer final : public std::enable_shared_from_this<RpcServer>,
                        private folly::AsyncServerSocket::AcceptCallback {
 public:
  /**
   * Create an RPC server.
   *
   * Request will be received on the passed EventBase and dispatched to the
   * RpcServerProcessor on the passed in threadPool.
   */
  static std::shared_ptr<RpcServer> create(
      std::shared_ptr<RpcServerProcessor> proc,
      folly::EventBase* evb,
      std::shared_ptr<folly::Executor> threadPool,
      const std::shared_ptr<StructuredLogger>& structuredLogger);

  /**
   * RpcServer must be torn down on its EventBase. destroy() is called by the
   * shared_ptr deleter.
   */
  void destroy();

  /**
   * Bind this server to the passed in address and start accepting
   * connections.
   */
  void initialize(folly::SocketAddress addr);

  /**
   * Initialize this server from an existing connected socket.
   */
  void initializeConnectedSocket(folly::File socket);

  /**
   * Initialize this server from an existing server socket.
   */
  void initializeServerSocket(folly::File socket);

  /**
   * Stop reading new requests, wait for pending requests, and detach and return
   * the connected socket for handoff to a new process.
   *
   * Must be called on the RpcServer's EventBase.
   */
  folly::SemiFuture<folly::File> takeoverStop();

  /**
   * Enables the service mapping with rpcbind/portmap.
   *
   * Must be called on the EventBase.
   */
  void registerService(uint32_t progNumber, uint32_t progVersion);

  /**
   * Return the EventBase that this RpcServer is running on.
   */
  folly::EventBase* getEventBase() const {
    return evb_;
  }

  /**
   * Returns the address that this RPC server is listening on.
   */
  folly::SocketAddress getAddr() const;

  /**
   * The socket underlying handlerToErase was closed and so the handler
   * is shutting down. This informs the server so that the server can stop
   * tracking it.
   *
   * Must be called on the EventBase.
   */
  void unregisterRpcHandler(RpcConnectionHandler* handlerToErase);

 private:
  RpcServer(
      std::shared_ptr<RpcServerProcessor> proc,
      folly::EventBase* evb,
      std::shared_ptr<folly::Executor> threadPool,
      const std::shared_ptr<StructuredLogger>& structuredLogger);

  ~RpcServer() override;

  // AsyncServerSocket::AcceptCallback

  void connectionAccepted(
      folly::NetworkSocket fd,
      const folly::SocketAddress& clientAddr,
      AcceptInfo /* info */) noexcept override;

  void acceptError(const std::exception& ex) noexcept override;

  void acceptStopped() noexcept override;

  // main event base that is used for socket interactions. Do not block this
  // event base, it needs to be available to process incoming reads and writes
  // on the socket.
  // TODO: This should probably be KeepAlive<EventBase>.
  folly::EventBase* evb_;

  // Threadpool for processing requests off the main event base.
  std::shared_ptr<folly::Executor> threadPool_;

  // Logger for logging anomalous things to Scuba
  std::shared_ptr<StructuredLogger> structuredLogger_;

  // listening socket for this server.
  folly::AsyncServerSocket::UniquePtr serverSocket_;

  // used to handle requests on the connected sockets.
  std::shared_ptr<RpcServerProcessor> proc_;

  struct PortmapState {
    PortmapClient portMap;
    std::vector<PortmapMapping4> mappedPorts;
  };

  struct State {
    // Set to true when acceptStopped() is called so we can assert that it was
    // synchronous.
    bool acceptStopped = false;

    std::optional<PortmapState> portmapState;

    // Existing handlers that have an open socket and are processing requests
    // from their socket.
    std::vector<RpcConnectionHandler::UniquePtr> connectionHandlers;
  };

  EventBaseState<State> state_;
};

} // namespace facebook::eden
