/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <vector>

#include <folly/SocketAddress.h>
#include <folly/io/async/AsyncServerSocket.h>
#include <folly/io/async/AsyncSocket.h>
#include <folly/net/NetworkSocket.h>

#include "eden/fs/nfs/portmap/PortmapClient.h"
#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/nfs/rpc/Server.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace folly {
class Executor;
}

namespace facebook::eden {

enum class RpcStopReason {
  RUNNING, // Running not stopping
  UNMOUNT, // happens when the socket is closed. For the nfsd3 the socket
           // closing means the mountpoint was unmounted (either eden
           // unmounted or force unmounted). For the mountd this means a normal
           // connection is closed, but we don't really care about this.
  ERROR, // happens when we encounter an error reading from the socket
  TAKEOVER,
};

struct RpcStopData {
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
  virtual void onShutdown(RpcStopData stopData);
};

class RpcServer;

class RpcTcpHandler : public folly::DelayedDestruction {
 public:
  using UniquePtr =
      std::unique_ptr<RpcTcpHandler, folly::DelayedDestruction::Destructor>;

  /**
   * Build a RpcTcpHandler.
   *
   * When the returned UniquePtr is dropped, this class will stay alive until
   * the client drops the connection, at which time the memory will be released
   * and the socket will be closed.
   */
  template <class... Args>
  static UniquePtr create(Args&&... args) {
    return UniquePtr(
        new RpcTcpHandler(std::forward<Args>(args)...),
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
  RpcTcpHandler(
      std::shared_ptr<RpcServerProcessor> proc,
      folly::AsyncSocket::UniquePtr&& socket,
      std::shared_ptr<folly::Executor> threadPool,
      std::weak_ptr<RpcServer> owningServer);

  class Reader : public folly::AsyncReader::ReadCallback {
   public:
    explicit Reader(RpcTcpHandler* handler);

    /**
     * This must be called on the main event base of the socket. This is
     * because we are going to access state_ which can only be accessed on the
     * main eventbase and we do operations on the socket (which generally can
     * only be done on the main eventbase).
     */
    folly::SemiFuture<folly::Unit> deleteMe(RpcStopReason stopReason);

   private:
    void getReadBuffer(void** bufP, size_t* lenP) override;

    void readDataAvailable(size_t len) noexcept override;

    bool isBufferMovable() noexcept override;

    void readBufferAvailable(
        std::unique_ptr<folly::IOBuf> readBuf) noexcept override;

    void readEOF() noexcept override;

    void readErr(const folly::AsyncSocketException& ex) noexcept override;

    RpcTcpHandler* handler_;
    DestructorGuard guard_;
  };

  class Writer : public folly::AsyncWriter::WriteCallback {
   public:
    Writer() = default;

   private:
    void writeSuccess() noexcept override {}

    void writeErr(
        size_t /*bytesWritten*/,
        const folly::AsyncSocketException& ex) noexcept override;
  };

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
   * Reads raw data off the socket.
   */
  std::unique_ptr<Reader> reader_;

  /**
   * Writes raw data to the socket.
   */
  Writer writer_{};

  folly::IOBufQueue readBuf_{folly::IOBufQueue::cacheChainLength()};

  /**
   * Status for the rpc connection. The State may only be accessed from the
   * socket's eventbase thread. We use this invariant so that we don't have to
   * add a lock to the state which would be used in the hot path for every
   * request.
   */
  struct State {
    // This is essentially equivelent to a status.
    RpcStopReason stopReason = RpcStopReason::RUNNING;
    // number of requests we are in the middle of processing
    size_t pendingRequests = 0;

    State() {}
    State(const State& state) = delete;
    State& operator=(const State&) = delete;
    State(State&& state) = delete;
    State& operator=(State&&) = delete;
  };

  /**
   * This wrapper exists to make it just a little bit harder to shoot yourself
   * in the foot and unknowingly access the state off the correct event base and
   * create a race condition.
   */
  class StateWrapper {
   public:
    explicit StateWrapper(folly::EventBase* evb) : evb_{evb}, state_{} {}
    State& get() {
      evb_->dcheckIsInEventBaseThread();
      return state_;
    }

   private:
    folly::EventBase* evb_;
    State state_;
  };

  StateWrapper state_;

  /**
   * Promise that we set during shutdown when we finish processing all the
   * pending requests
   */
  folly::Promise<folly::Unit> pendingRequestsComplete_;

  /**
   * RpcServer that initiated this RpcTcpHandler. We keep a reference to this
   * so that we can notify the server when we are shutting down. The server
   * should outlive all of it's connections, but if the server has already been
   * shutdown then we can just skip notifying it that we are shutting down.
   */
  std::weak_ptr<RpcServer> owningServer_;
};

class RpcServer : public std::enable_shared_from_this<RpcServer> {
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
      std::shared_ptr<folly::Executor> threadPool);

  ~RpcServer();

  /**
   * Bind this server to the passed in address and start accepting
   * connections.
   */
  void initialize(folly::SocketAddress addr);

  enum class InitialSocketType { SERVER_SOCKET, CONNECTED_SOCKET };

  /**
   * Initialize this server from an already existing socket. connected indicates
   * if this is a connected socket or server socket.
   */
  void initialize(folly::File&& socket, InitialSocketType type);

  folly::SemiFuture<folly::File> takeoverStop();

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
   * A client attempted to connect to this server and spawned a handler
   * this is used to inform the server about the handler so that it can manage
   * it.
   */
  void registerRpcHandler(RpcTcpHandler::UniquePtr handler);

  /**
   * The socket underlying handlerToErase was closed and so the handler
   * is shutting down. This informs the server so that the server can stop
   * tracking it.
   */
  void unregisterRpcHandler(RpcTcpHandler* handlerToErase);

 private:
  RpcServer(
      std::shared_ptr<RpcServerProcessor> proc,
      folly::EventBase* evb,
      std::shared_ptr<folly::Executor> threadPool);

  class RpcAcceptCallback : public folly::AsyncServerSocket::AcceptCallback,
                            public folly::DelayedDestruction {
   public:
    using UniquePtr = std::
        unique_ptr<RpcAcceptCallback, folly::DelayedDestruction::Destructor>;

    explicit RpcAcceptCallback(
        std::shared_ptr<RpcServerProcessor> proc,
        folly::EventBase* evb,
        std::shared_ptr<folly::Executor> threadPool,
        std::weak_ptr<RpcServer> owningServer)
        : evb_(evb),
          proc_(proc),
          threadPool_(std::move(threadPool)),
          owningServer_(std::move(owningServer)),
          guard_(this) {}

   private:
    void connectionAccepted(
        folly::NetworkSocket fd,
        const folly::SocketAddress& clientAddr,
        AcceptInfo /* info */) noexcept override;

    void acceptError(const std::exception& ex) noexcept override;

    void acceptStopped() noexcept override;

    ~RpcAcceptCallback() override = default;

    folly::EventBase* evb_;
    std::shared_ptr<RpcServerProcessor> proc_;
    std::shared_ptr<folly::Executor> threadPool_;
    std::weak_ptr<RpcServer> owningServer_;

    /**
     * Hold a guard to ourself to avoid being deleted until the callback is
     * removed from the AsyncServerSocket.
     */
    std::optional<folly::DelayedDestruction::DestructorGuard> guard_;
  };

  // main event base that is used for socket interactions. Do not block this
  // event base, it needs to be available to process incoming reads and writes
  // on the socket.
  folly::EventBase* evb_;

  // Threadpool for processing requests off the main event base.
  std::shared_ptr<folly::Executor> threadPool_;

  // will be called when clients connect to the server socket.
  RpcAcceptCallback::UniquePtr acceptCb_;

  // listening socket for this server.
  folly::AsyncServerSocket::UniquePtr serverSocket_;

  // used to handle requests on the connected sockets.
  std::shared_ptr<RpcServerProcessor> proc_;

  struct PortmapState {
    PortmapState() = default;

    PortmapClient portMap;
    std::vector<PortmapMapping> mappedPorts;
  };
  folly::Synchronized<std::optional<PortmapState>> portMapState_;

  // Existing handlers that have an open socket and are processing requests from
  // their socket.
  folly::Synchronized<std::vector<RpcTcpHandler::UniquePtr>> rpcTcpHandlers_;
};

} // namespace facebook::eden

#endif
