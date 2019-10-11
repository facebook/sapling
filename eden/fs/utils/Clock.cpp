/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32
#include "folly/portability/Time.h"
#endif

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

} // namespace eden
} // namespace facebook
