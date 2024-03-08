/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <fb303/detail/QuantileStatWrappers.h>
#include <folly/ThreadLocal.h>

namespace facebook::eden {

/**
 * StatsGroupBase is a base class for a group of thread-local stats
 * structures.
 *
 * Each StatsGroupBase object should only be used from a single thread. The
 * EdenStats object should be used to maintain one StatsGroupBase object
 * for each thread that needs to access/update the stats.
 */
class StatsGroupBase {
  using Stat = fb303::detail::QuantileStatWrapper;

 public:
  /**
   * Counter is used to record events.
   */
  class Counter : private Stat {
   public:
    explicit Counter(std::string_view name);

    using Stat::addValue;
  };

  /**
   * Duration is used for stats that measure elapsed times.
   *
   * In general, EdenFS measures latencies in units of microseconds.
   * Duration enforces that its stat names end in "_us".
   */
  class Duration : private Stat {
   public:
    explicit Duration(std::string_view name);

    /**
     * Record a duration in microseconds to the QuantileStatWrapper. Also
     * increments the .count statistic.
     */
    template <typename Rep, typename Period>
    void addDuration(std::chrono::duration<Rep, Period> elapsed) {
      // TODO: Implement a general overflow check when converting from seconds
      // or milliseconds to microseconds. Fortunately, this use case deals with
      // short durations.
      addDuration(
          std::chrono::duration_cast<std::chrono::microseconds>(elapsed));
    }

    void addDuration(std::chrono::microseconds elapsed);
  };
};

template <typename T>
class StatsGroup : public StatsGroupBase {
 public:
  /**
   * Statistics are often updated on a thread separate from the thread that
   * started a request. Since stat objects are thread-local, we cannot hold
   * pointers directly to them. Instead, we store a pointer-to-member and look
   * up the calling thread's object.
   */
  using DurationPtr = Duration T::*;
};

} // namespace facebook::eden
