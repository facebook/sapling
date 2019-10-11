/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
