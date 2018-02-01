/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <time.h>

struct fuse_setattr_in;

namespace facebook {
namespace eden {

class Clock;

/**
 * Structure for wrapping atime,ctime,mtime
 */
struct InodeTimestamps {
  // TODO: As a future optimization, each of these could be packed into 64-bit
  // nanoseconds from UNIX epoch to year 2500 or something.
  timespec atime{};
  timespec mtime{};
  timespec ctime{};

  /**
   * Assigns the specified ts to atime, mtime, and ctime.
   */
  void setAll(const timespec& ts) {
    atime = ts;
    mtime = ts;
    ctime = ts;
  }

  /**
   * Helper that assigns all three timestamps from the flags and parameters in
   * a fuse_setattr_in struct.
   *
   * Always sets ctime to the current time as given by the clock.
   */
  void setattrTimes(const Clock& clock, const fuse_setattr_in& attr);
};

} // namespace eden
} // namespace facebook
