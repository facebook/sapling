/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace folly {
template <typename T>
class Future;
}

namespace facebook::eden {

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
  virtual ~TakeoverHandler() = default;

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

  virtual void closeStorage() = 0;

  /*
   * This is a temporary function that override
   * TakeoverCapabilities::CHUNKED_MESSAGE. We use it to control the protocol
   * roll out through Eden config. This function should removed after the roll
   * out has completed.
   */
  virtual bool shouldChunkTakeoverData() = 0;
};

} // namespace facebook::eden
