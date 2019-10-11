/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FutureUnixSocket.h"

#include <folly/SocketAddress.h>
#include <folly/io/async/AsyncTimeout.h>
#include <folly/logging/xlog.h>

using folly::exception_wrapper;
using folly::Future;
using folly::make_exception_wrapper;
using folly::makeFuture;
using folly::Promise;
using folly::Unit;

namespace facebook {
namespace eden {

class FutureUnixSocket::SendCallback : public UnixSocket::SendCallback {
 public:
  SendCallback() {}

  Future<Unit> getFuture() {
    return promise_.getFuture();
  }

  void sendSuccess() noexcept override {
    promise_.setValue();
    delete this;
  }
  void sendError(const exception_wrapper& ew) noexcept override {
    promise_.setException(ew);
    delete this;
  }

 private:
  Promise<Unit> promise_;
};

class FutureUnixSocket::ReceiveCallback : public folly::AsyncTimeout {
 public:
  explicit ReceiveCallback(FutureUnixSocket* socket)
      : folly::AsyncTimeout{socket->getEventBase()}, socket_{socket} {}

  Future<Message> getFuture() {
    return promise_.getFuture();
  }

  std::unique_ptr<ReceiveCallback> popNext() {
    return std::move(next_);
  }
  void append(std::unique_ptr<ReceiveCallback> next) {
    CHECK(!next_);
    next_ = std::move(next);
  }

  void setValue(Message&& message) {
    promise_.setValue(std::move(message));
  }
  void setException(const exception_wrapper& ew) {
    promise_.setException(ew);
  }

  void timeoutExpired() noexcept override {
    socket_->receiveTimeout();
  }

 private:
  FutureUnixSocket* const socket_{nullptr};
  std::unique_ptr<ReceiveCallback> next_{nullptr};
  Promise<Message> promise_;
};

class FutureUnixSocket::ConnectCallback : public UnixSocket::ConnectCallback {
 public:
  explicit ConnectCallback(FutureUnixSocket* socket) : socket_{socket} {}

  Future<Unit> getFuture() {
    return promise_.getFuture();
  }

  void connectSuccess(UnixSocket::UniquePtr socket) noexcept override {
    *socket_ = FutureUnixSocket{std::move(socket)};
    promise_.setValue();
    delete this;
  }
  void connectError(folly::exception_wrapper&& ew) noexcept override {
    promise_.setException(ew);
    delete this;
  }

 private:
  FutureUnixSocket* socket_{nullptr};
  Promise<Unit> promise_;
};

FutureUnixSocket::FutureUnixSocket() {}

FutureUnixSocket::FutureUnixSocket(UnixSocket::UniquePtr socket)
    : socket_{std::move(socket)} {}

FutureUnixSocket::FutureUnixSocket(
    folly::EventBase* eventBase,
    folly::File socket)
    : socket_{UnixSocket::makeUnique(eventBase, std::move(socket))} {}

FutureUnixSocket::FutureUnixSocket(FutureUnixSocket&& other) noexcept
    : socket_{std::move(other.socket_)},
      recvQueue_{std::move(other.recvQueue_)},
      recvQueueTail_{other.recvQueueTail_} {
  other.recvQueueTail_ = nullptr;
}

FutureUnixSocket& FutureUnixSocket::operator=(
    FutureUnixSocket&& other) noexcept {
  socket_ = std::move(other.socket_);
  recvQueue_ = std::move(other.recvQueue_);
  recvQueueTail_ = other.recvQueueTail_;
  other.recvQueueTail_ = nullptr;
  return *this;
}

FutureUnixSocket::~FutureUnixSocket() {
  if (socket_) {
    socket_->closeNow();
  }
  // closeNow() should have forced us to clear out recvQueue_
  CHECK(!recvQueue_);
  CHECK(!recvQueueTail_);
}

Future<Unit> FutureUnixSocket::connect(
    folly::EventBase* eventBase,
    folly::StringPiece path,
    std::chrono::milliseconds timeout) {
  folly::SocketAddress address;
  address.setFromPath(path);
  return connect(eventBase, address, timeout);
}

Future<Unit> FutureUnixSocket::connect(
    folly::EventBase* eventBase,
    const folly::SocketAddress& address,
    std::chrono::milliseconds timeout) {
  auto callback = new ConnectCallback{this};
  auto future = callback->getFuture();
  UnixSocket::connect(callback, eventBase, address, timeout);
  return future;
}

uid_t FutureUnixSocket::getRemoteUID() {
  if (!socket_) {
    throw std::runtime_error("cannot get the UID of a closed socket");
  }
  return socket_->getRemoteUID();
}

void FutureUnixSocket::closeNow() {
  socket_.reset();
}

Future<Unit> FutureUnixSocket::send(Message&& msg) {
  if (!socket_) {
    return makeFuture<Unit>(
        std::runtime_error("cannot send on a closed socket"));
  }
  auto* callback = new SendCallback();
  auto future = callback->getFuture();
  socket_->send(std::move(msg), callback);
  return future;
}

Future<UnixSocket::Message> FutureUnixSocket::receive(
    std::chrono::milliseconds timeout) {
  if (!socket_) {
    return makeFuture<Message>(
        std::runtime_error("cannot receive on a closed socket"));
  }

  auto callback = std::make_unique<ReceiveCallback>(this);
  auto future = callback->getFuture();
  callback->scheduleTimeout(timeout);

  auto previousTail = recvQueueTail_;
  recvQueueTail_ = callback.get();
  if (previousTail) {
    DCHECK(recvQueue_);
    previousTail->append(std::move(callback));
  } else {
    DCHECK(!recvQueue_);
    recvQueue_ = std::move(callback);
    socket_->setReceiveCallback(this);
  }

  return future;
}

void FutureUnixSocket::receiveTimeout() {
  // Save all of the receive promises so we can fail them with
  // a timeout error.
  auto q = std::move(recvQueue_);
  recvQueue_ = nullptr;
  recvQueueTail_ = nullptr;

  // Close and destroy the underlying socket.
  socket_.reset();

  auto error = make_exception_wrapper<std::system_error>(
      ETIMEDOUT, std::generic_category(), "receive timeout on unix socket");
  failReceiveQueue(std::move(q), error);
}

void FutureUnixSocket::messageReceived(Message&& message) noexcept {
  XLOG(DBG3) << "messageReceived()";
  CHECK(recvQueue_);
  DCHECK(recvQueueTail_);

  auto callback = std::move(recvQueue_);
  recvQueue_ = callback->popNext();
  if (!recvQueue_) {
    recvQueueTail_ = nullptr;
    socket_->clearReceiveCallback();
  } else {
    DCHECK(recvQueueTail_);
    DCHECK_NE(recvQueueTail_, callback.get());
  }

  // Fulfill the callback as the very last thing we do,
  // in case it destroys us.
  callback->setValue(std::move(message));
}

void FutureUnixSocket::eofReceived() noexcept {
  XLOG(DBG3) << "eofReceived()";
  socket_.reset();
  failAllPromises(std::runtime_error("remote endpoint closed connection"));
}

void FutureUnixSocket::socketClosed() noexcept {
  XLOG(DBG3) << "socketClosed()";
  socket_.reset();
  failAllPromises(std::runtime_error("socket closed locally"));
}

void FutureUnixSocket::receiveError(const exception_wrapper& ew) noexcept {
  XLOG(DBG3) << "receiveError()";
  socket_.reset();
  failAllPromises(ew);
}

void FutureUnixSocket::failAllPromises(
    const exception_wrapper& error) noexcept {
  auto q = std::move(recvQueue_);
  recvQueue_ = nullptr;
  recvQueueTail_ = nullptr;
  failReceiveQueue(std::move(q), error);
}

void FutureUnixSocket::failReceiveQueue(
    std::unique_ptr<ReceiveCallback> callback,
    const exception_wrapper& ew) {
  while (callback) {
    auto next = callback->popNext();
    callback->setException(ew);
    callback = std::move(next);
  }
}

} // namespace eden
} // namespace facebook
