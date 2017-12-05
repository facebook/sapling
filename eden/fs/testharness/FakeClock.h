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

  timespec getRealtime() override {
    return folly::to<timespec>(currentTime);
  }

  clock::time_point getTimePoint() const {
    return currentTime;
  }

  void set(clock::time_point to) {
    currentTime = to;
  }

  void advance(clock::duration by) {
    currentTime += by;
  }

 private:
  clock::time_point currentTime;
};

} // namespace eden
} // namespace facebook
