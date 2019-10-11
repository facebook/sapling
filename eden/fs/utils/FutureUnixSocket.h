/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>

#include "eden/fs/utils/UnixSocket.h"

namespace facebook {
namespace eden {

/**
 * A wrapper around UnixSocket that provides a Future-based API
 * rather than raw callback objects.
 *
 * This class is not thread safe.  It should only be accessed from the
 * EventBase thread that it is attached to.
 */
class FutureUnixSocket : private UnixSocket::ReceiveCallback {
 public:
  using Message = UnixSocket::Message;

  /**
   * Create a new unconnected FutureUnixSocket object.
   *
   * connect() should be called on this socket before any other I/O operations.
   */
  FutureUnixSocket();

  /**
   * Create a FutureUnixSocket object from an existing UnixSocket.
   */
  explicit FutureUnixSocket(UnixSocket::UniquePtr socket);

  /**
   * Create a FutureUnixSocket object from an existing socket descriptor.
   */
  FutureUnixSocket(folly::EventBase* eventBase, folly::File socket);

  ~FutureUnixSocket();
  FutureUnixSocket(FutureUnixSocket&& other) noexcept;
  FutureUnixSocket& operator=(FutureUnixSocket&& other) noexcept;

  /**
   * Connect to a unix socket.
   */
  folly::Future<folly::Unit> connect(
      folly::EventBase* eventBase,
      const folly::SocketAddress& address,
      std::chrono::milliseconds timeout);
  folly::Future<folly::Unit> connect(
      folly::EventBase* eventBase,
      folly::StringPiece path,
      std::chrono::milliseconds timeout);

  /**
   * Get the EventBase that this socket uses for driving I/O operations.
   *
   * All interaction with this FutureUnixSocket object must be done from this
   * EventBase's thread.
   */
  folly::EventBase* getEventBase() const {
    return socket_->getEventBase();
  }

  /**
   * Attach this socket to an EventBase.
   *
   * This should only be called to set the EventBase if the UnixSocket
   * constructor was called with a null EventBase.  If the EventBase was not
   * set in the constructor then attachEventBase() must be called before any
   * calls to send() or setReceiveCallback().
   *
   * This method may only be called from the EventBase's thread.  If the
   * EventBase has not been started yet it may be called from another thread if
   * that thread is the only thread accessing the EventBase.
   */
  void attachEventBase(folly::EventBase* eventBase) {
    socket_->attachEventBase(eventBase);
  }

  /**
   * Detach from the EventBase that is being used to drive this socket.
   *
   * This may only be called from the EventBase thread.
   */
  void detachEventBase() {
    socket_->detachEventBase();
  }

  void setSendTimeout(std::chrono::milliseconds timeout) {
    return socket_->setSendTimeout(timeout);
  }

  /**
   * Returns 'true' if the underlying descriptor is open, or rather,
   * it has not been closed locally.
   */
  explicit operator bool() const {
    return socket_.get() != nullptr;
  }

  /**
   * Close the socket immediately.
   *
   * This aborts any send() and receive() calls that are in progress.
   */
  void closeNow();

  /**
   * Get the user ID of the remote peer.
   */
  uid_t getRemoteUID();

  /**
   * Send a message.
   *
   * Returns a Future that will complete when the message has been handed off
   * to the kernel for delivery.
   */
  folly::Future<folly::Unit> send(Message&& msg);
  folly::Future<folly::Unit> send(folly::IOBuf&& data) {
    return send(Message(std::move(data)));
  }
  folly::Future<folly::Unit> send(std::unique_ptr<folly::IOBuf> data) {
    return send(Message(std::move(*data)));
  }

  /**
   * Receive a message.
   *
   * Returns a Future that will be fulfilled when a message is received.
   *
   * receive() may be called multiple times in a row without waiting for
   * earlier receive() calls to be fulfilled.  In this case the futures will be
   * fulfilled as messages are received in the order in which they were
   * created.  (The first receive() call will receive the first message
   * received on the socket, the second receive() call will receive the second
   * message, etc.)
   */
  folly::Future<Message> receive(std::chrono::milliseconds timeout);

 private:
  class SendCallback;
  class ReceiveCallback;
  class ConnectCallback;

  void receiveTimeout();

  void messageReceived(Message&& message) noexcept override;
  void eofReceived() noexcept override;
  void socketClosed() noexcept override;
  void receiveError(const folly::exception_wrapper& ew) noexcept override;

  void failAllPromises(const folly::exception_wrapper& error) noexcept;
  static void failReceiveQueue(
      std::unique_ptr<ReceiveCallback> callback,
      const folly::exception_wrapper& ew);

  UnixSocket::UniquePtr socket_;
  std::unique_ptr<ReceiveCallback> recvQueue_;
  ReceiveCallback* recvQueueTail_{nullptr};
};

} // namespace eden
} // namespace facebook
