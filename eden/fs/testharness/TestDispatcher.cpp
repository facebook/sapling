/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/testharness/TestDispatcher.h"

#include <folly/Conv.h>
#include <folly/logging/xlog.h>

#include "eden/fs/fuse/RequestData.h"

using folly::Future;
using std::string;

namespace facebook {
namespace eden {

Future<fuse_entry_out> TestDispatcher::lookup(
    InodeNumber parent,
    PathComponentPiece name) {
  auto requestID = RequestData::get().getReq().unique;
  XLOG(DBG5) << "received lookup " << requestID << ": parent=" << parent
             << ", name=" << name;
  auto result = Future<fuse_entry_out>::makeEmpty();
  {
    // Whenever we receive a lookup request just add it to the pendingLookups_
    // The test harness can then respond to it later however it wants.
    auto state = state_.lock();
    auto emplaceResult =
        state->pendingLookups.emplace(requestID, PendingLookup(parent, name));

    // We expect the test code to generate unique request IDs,
    // just like the kernel should.
    CHECK(emplaceResult.second) << "received duplicate request ID " << requestID
                                << " from the test harness";
    result = emplaceResult.first->second.promise.getFuture();
  }

  requestReceived_.notify_all();
  return result;
}

TestDispatcher::PendingLookup TestDispatcher::waitForLookup(
    uint64_t requestId,
    std::chrono::milliseconds timeout) {
  auto state = state_.lock();
  auto end_time = std::chrono::steady_clock::now() + timeout;
  while (true) {
    auto iter = state->pendingLookups.find(requestId);
    if (iter != state->pendingLookups.end()) {
      PendingLookup result(std::move(iter->second));
      state->pendingLookups.erase(iter);
      return result;
    }

    if (requestReceived_.wait_until(state.getUniqueLock(), end_time) ==
        std::cv_status::timeout) {
      throw std::runtime_error(folly::to<string>(
          "timed out waiting for test dispatcher to receive lookup request ",
          requestId));
    }
  }
}
} // namespace eden
} // namespace facebook
