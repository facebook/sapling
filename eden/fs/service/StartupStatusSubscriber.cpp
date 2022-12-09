/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/StartupStatusSubscriber.h"

#include <folly/logging/xlog.h>

#include "eden/fs/utils/EdenError.h"

namespace facebook::eden {

void StartupStatusChannel::subscribe(
    std::unique_ptr<StartupStatusSubscriber> subscriber) {
  {
    auto state = state_.lock();
    if (!state->subscribersClosed) {
      state->subscribers.push_back(std::move(subscriber));
      return;
    }
  }

  // if we fell through then we did not add to the publishers list because
  // startup has already completed. The publisher will be automaticall destroyed
  // as we go out of scope. We throw an error to indicate startup has completed.
  throw newEdenError(
      EALREADY,
      EdenErrorType::POSIX_ERROR,
      "EdenFS has already started. No startup status available.");
}

void StartupStatusChannel::startupCompleted() {
  std::vector<std::unique_ptr<StartupStatusSubscriber>> toDestroy;
  {
    auto state = state_.lock();
    if (state->subscribersClosed) {
      // the already closed the publishers because eden was shut down while
      // starting.
      XCHECK(state->subscribers.empty());
      return;
    }
    // destructing the publishers signals to them that startup has completed.
    toDestroy.swap(state->subscribers);
    state->subscribersClosed = true;
  }
}

void StartupStatusChannel::publish(std::string_view data) {
  {
    auto state = state_.lock();
    if (!state->subscribersClosed) {
      for (auto& subscriber : state->subscribers) {
        // notice we are holding the lock here which is a deadlock risk
        subscriber->publish(data);
      }
    }
  }
}
} // namespace facebook::eden
