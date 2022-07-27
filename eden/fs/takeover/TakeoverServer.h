/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/io/async/AsyncServerSocket.h>
#include <memory>

#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

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
      TakeoverHandler* handler,
      FaultInjector* FOLLY_NONNULL faultInjector,
      const std::set<int32_t>& supportedVersions = kSupportedTakeoverVersions,
      const uint64_t supportedCapabilities = kSupportedCapabilities);
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
      const folly::SocketAddress& clientAddr,
      AcceptInfo /* info */) noexcept override;
  void acceptError(folly::exception_wrapper ex) noexcept override;

  void connectionDone(ConnHandler* handler);

  folly::EventBase* eventBase_{nullptr};
  TakeoverHandler* handler_{nullptr};
  AbsolutePath socketPath_;
  folly::AsyncServerSocket::UniquePtr socket_;
  FaultInjector& faultInjector_;
  // generally this should be kSupportedCapabilities, but we allow setting
  // it differently, mostly for tests so that you can test capabilities that
  // might not be ready for production yet.
  const uint64_t supportedCapabilities_;
  // same goes for versions even though they are on the way out.
  const std::set<int32_t>& supportedVersions_;
};
} // namespace facebook::eden
