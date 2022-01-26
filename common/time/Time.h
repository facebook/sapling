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

#include <time.h>

namespace facebook {

// Stub.  Should probably change calling code to use folly chrono apis instead
class WallClockUtil {
 public:
  // ----------------  time in seconds  ---------------
  static time_t NowInSecSlow() {
    return ::time(nullptr);
  }
  static time_t NowInSecFast() {
    return ::time(nullptr);
  }
};

} // namespace facebook
