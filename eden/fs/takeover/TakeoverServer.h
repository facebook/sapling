/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
      folly::NetworkSocket fdNetworkSocket,
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
