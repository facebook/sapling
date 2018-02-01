/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <time.h>

namespace facebook {
namespace eden {

/**
 * Represents access to the system clock(s).
 */
class Clock {
 public:
  virtual ~Clock() {}

  /**
   * Returns the real time elapsed since the Epoch.
   */
  virtual timespec getRealtime() const = 0;
};

/**
 *
 */
class UnixClock : public Clock {
 public:
  /// CLOCK_REALTIME
  timespec getRealtime() const override;
};

} // namespace eden
} // namespace facebook
