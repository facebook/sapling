/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32
#include "folly/portability/Time.h"
#endif

#include <chrono>
#include <system_error>
#include "Clock.h"

namespace facebook {
namespace eden {

timespec UnixClock::getRealtime() const {
  timespec rv;
  if (clock_gettime(CLOCK_REALTIME, &rv)) {
    throw std::system_error(
        errno, std::generic_category(), "clock_gettime failed");
  }
  return rv;
}

float UnixClock::getElapsedTimeInNs(timespec startTime, timespec currTime) {
  auto currDuration = std::chrono::duration_cast<std::chrono::nanoseconds>(
      std::chrono::seconds{currTime.tv_sec} +
      std::chrono::nanoseconds{currTime.tv_nsec});
  auto startDuration = std::chrono::duration_cast<std::chrono::nanoseconds>(
      std::chrono::seconds{startTime.tv_sec} +
      std::chrono::nanoseconds{startTime.tv_nsec});
  float uptime = float((currDuration - startDuration).count()) / 1000000000L;
  return uptime;
}

} // namespace eden
} // namespace facebook
