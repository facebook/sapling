/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
