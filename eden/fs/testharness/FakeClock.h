/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include "eden/fs/utils/Clock.h"

#include <folly/chrono/Conv.h>

namespace facebook {
namespace eden {

class FakeClock : public Clock {
 public:
  using clock = std::chrono::system_clock;
  using time_point = clock::time_point;
  using duration = clock::duration;

  timespec getRealtime() const override {
    return folly::to<timespec>(currentTime_);
  }

  time_point getTimePoint() const {
    return currentTime_;
  }

  void set(time_point to) {
    currentTime_ = to;
  }

  void advance(duration by) {
    currentTime_ += by;
  }

 private:
  time_point currentTime_;
};

} // namespace eden
} // namespace facebook
