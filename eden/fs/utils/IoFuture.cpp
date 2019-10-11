/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/IoFuture.h"

#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/io/async/AsyncTimeout.h>
#include <folly/io/async/EventHandler.h>

using folly::EventBase;
using folly::Promise;
using std::make_unique;

namespace facebook {
namespace eden {

IoFuture::IoFuture(folly::EventBase* eventBase, int socket)
    : EventHandler{eventBase, folly::NetworkSocket::fromFd(socket)},
      AsyncTimeout{eventBase},
      promise_{Promise<int>::makeEmpty()} {}

folly::Future<int> IoFuture::wait(
    int eventFlags,
    folly::TimeoutManager::timeout_type timeout) {
  // wait() may be called multiple times.
  // If someone is calling wait() again before the previous wait() finished,
  // fail the previously returned future with an exception indicating that the
  // I/O wait was canceled.
  if (!promise_.isFulfilled()) {
    promise_.setException(std::system_error(
        ECANCELED, std::generic_category(), "I/O wait canceled"));
  }
  promise_ = Promise<int>{};

  // We do not support using the EventHandler::PERSIST flag.
  // folly::Future objects are one-shot, so it doesn't make sense to repeatedly
  // wait for I/O to be ready using a Future.
  CHECK(!(eventFlags & EventHandler::PERSIST));

  auto future = promise_.getFuture();

  // Register a timeout in case the remote side does not send
  // credentials soon.
  if (!scheduleTimeout(timeout)) {
    promise_.setException(std::system_error(
        EIO, std::generic_category(), "error registering for socket I/O"));
    return future;
  }

  // Register for the requested I/O event
  if (!registerHandler(eventFlags)) {
    promise_.setException(std::system_error(
        EIO, std::generic_category(), "error registering for socket I/O"));
    return future;
  }

  return future;
}

void IoFuture::handlerReady(uint16_t events) noexcept {
  cancelTimeout();
  promise_.setValue(events);
}

void IoFuture::timeoutExpired() noexcept {
  unregisterHandler();
  promise_.setException(std::system_error(
      ETIMEDOUT, std::generic_category(), "timed out waiting for socket I/O"));
}

folly::Future<int> waitForIO(
    EventBase* eventBase,
    int socket,
    int eventFlags,
    folly::TimeoutManager::timeout_type timeout) {
  return folly::makeFutureWith([&] {
    auto ioFuture = make_unique<IoFuture>(eventBase, socket);
    auto f = ioFuture.get();
    return f->wait(eventFlags, timeout).ensure([iof = std::move(ioFuture)] {});
  });
}

} // namespace eden
} // namespace facebook
