/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Conv.h>
#include <folly/Portability.h>
#include <folly/futures/Promise.h>
#include <folly/io/async/AsyncTimeout.h>
#include <folly/io/async/EventHandler.h>

namespace folly {
class EventBase;
}

namespace facebook {
namespace eden {

/**
 * Create a Future that will complete when a socket is ready to perform I/O.
 *
 * The eventFlags parameter is a set of folly::EventHandler::EventFlags flags,
 * the same used by EventHandler::registerHandler().
 *
 * The EventHandler::PERSIST flag must not be set in the input flags: a Future
 * object is a one-shot event, so it cannot be used to repeatedly wait for I/O
 * notifications.
 *
 * The returned Future will return the EventHandler::EventFlags that are now
 * ready.
 */
FOLLY_NODISCARD folly::Future<int> waitForIO(
    folly::EventBase* eventBase,
    int socket,
    int eventFlags,
    folly::TimeoutManager::timeout_type timeout);

/**
 * A helper class to provide a folly::Future that completes when a socket is
 * ready for I/O.
 *
 * This is similar to use waitForIO(), but can be re-used multiple times if you
 * need to repeatedly wait for I/O.
 */
class IoFuture : private folly::EventHandler, private folly::AsyncTimeout {
 public:
  IoFuture(folly::EventBase* eventBase, int socket);

  /**
   * Wait for I/O to be ready on the socket.
   *
   * The eventFlags parameter is a set of folly::EventHandler::EventFlags flags,
   * the same used by EventHandler::registerHandler().
   *
   * wait() can be called multiple times on the same IoFuture, but subsequent
   * calls to wait() will interrupt any previous wait() call that has not
   * already completed.  If this occurs the Future returned by the previous
   * wait() call will be failed with an exception.
   */
  FOLLY_NODISCARD folly::Future<int> wait(
      int eventFlags,
      folly::TimeoutManager::timeout_type timeout);

 private:
  void handlerReady(uint16_t events) noexcept override;
  void timeoutExpired() noexcept override;

  folly::Promise<int> promise_;
};

} // namespace eden
} // namespace facebook
