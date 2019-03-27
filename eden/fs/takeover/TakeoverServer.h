/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/io/async/AsyncServerSocket.h>
#include <memory>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class TakeoverData;
class TakeoverHandler;

/**
 * A helper class that listens on a unix domain socket for clients that
 * wish to perform graceful takeover of this EdenServer's mount points.
 */
class TakeoverServer : private folly::AsyncServerSocket::AcceptCallback {
 public:
  explicit TakeoverServer(
      folly::EventBase* eventBase,
      AbsolutePathPiece socketPath,
      TakeoverHandler* handler);
  virtual ~TakeoverServer() override;

  void start();

  TakeoverHandler* getTakeoverHandler() const {
    return handler_;
  }

 private:
  class ConnHandler;

  folly::EventBase* getEventBase() const {
    return eventBase_;
  }

  // AcceptCallback methods
  void connectionAccepted(
      int fd,
      const folly::SocketAddress& clientAddr) noexcept override;
  void acceptError(const std::exception& ex) noexcept override;

  void connectionDone(ConnHandler* handler);

  folly::EventBase* eventBase_{nullptr};
  TakeoverHandler* handler_{nullptr};
  AbsolutePath socketPath_;
  folly::AsyncServerSocket::UniquePtr socket_;
};
} // namespace eden
} // namespace facebook
