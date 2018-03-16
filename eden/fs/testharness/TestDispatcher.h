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

#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <chrono>
#include <condition_variable>
#include <unordered_map>

#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * A FUSE Dispatcher implementation for use in unit tests.
 *
 * It allows the test code to generate responses to specific requests on
 * demand.
 */
class TestDispatcher : public fusell::Dispatcher {
 public:
  /**
   * Data for a pending FUSE_LOOKUP request.
   */
  struct PendingLookup {
    PendingLookup(fusell::InodeNumber parent, PathComponentPiece name)
        : parent(parent), name(name.copy()) {}

    fusell::InodeNumber parent;
    PathComponent name;
    folly::Promise<fuse_entry_out> promise;
  };

  using Dispatcher::Dispatcher;

  folly::Future<fuse_entry_out> lookup(
      fusell::InodeNumber parent,
      PathComponentPiece name) override;

  /**
   * Wait for the dispatcher to receive a FUSE_LOOKUP request with the
   * specified request ID.
   *
   * Returns a PendingLookup object that can be used to respond to the request.
   */
  PendingLookup waitForLookup(
      uint64_t requestId,
      std::chrono::milliseconds timeout = std::chrono::milliseconds(500));

 private:
  struct State {
    std::unordered_map<uint64_t, PendingLookup> pendingLookups;
  };

  folly::Synchronized<State, std::mutex> state_;
  std::condition_variable requestReceived_;
};

} // namespace eden
} // namespace facebook
