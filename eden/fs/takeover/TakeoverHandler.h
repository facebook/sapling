/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {

class TakeoverData;

/**
 * TakeoverHandler is a pure virtual interface for classes that want to
 * implement graceful takeover functionality.
 *
 * This is primarily implemented by the EdenServer class.  However, there are
 * also alternative implementations used for unit testing.
 */
class TakeoverHandler {
 public:
  virtual ~TakeoverHandler() {}

  /**
   * startTakeoverShutdown() will be called when a graceful shutdown has been
   * requested, with a remote process attempting to take over the currently
   * running mount points.
   *
   * This should return a Future that will produce the TakeoverData to send to
   * the remote edenfs process once this edenfs process is ready to transfer
   * its mounts.
   */
  virtual folly::Future<TakeoverData> startTakeoverShutdown() = 0;
};

} // namespace eden
} // namespace facebook
