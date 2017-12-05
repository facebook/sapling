/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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

  timespec getRealtime() override {
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
