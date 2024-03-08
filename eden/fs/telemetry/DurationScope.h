/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <memory>

#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>

#include "eden/fs/telemetry/StatsGroup.h"

namespace facebook::eden {

/**
 * On construction, notes the current time. On destruction, records the elapsed
 * time in the specified Stats Duration.
 *
 * Moveable, but not copyable.
 */
template <typename Stats>
class DurationScope {
 public:
  using StatsPtr = RefPtr<Stats>;

  DurationScope() = delete;

  template <typename T>
  DurationScope(StatsPtr&& stats, StatsGroupBase::Duration T::*duration)
      : stats_{std::move(stats)},
        // This use of std::function won't allocate on libstdc++,
        // libc++, or Microsoft STL. All three have a couple pointers
        // worth of small buffer inline storage.
        updateScope_{[duration](Stats& stats, StopWatch::duration elapsed) {
          stats.addDuration(duration, elapsed);
        }} {
    assert(stats_);
  }

  template <typename T>
  DurationScope(const StatsPtr& stats, StatsGroupBase::Duration T::*duration)
      : DurationScope{stats.copy(), duration} {}

  ~DurationScope() noexcept {
    if (stats_ && updateScope_) {
      try {
        updateScope_(*stats_, stopWatch_.elapsed());
      } catch (const std::exception& e) {
        XLOG(ERR) << "error recording duration: " << e.what();
      }
    }
  }

  DurationScope(DurationScope&& that) = default;
  DurationScope& operator=(DurationScope&& that) = default;

  DurationScope(const DurationScope&) = delete;
  DurationScope& operator=(const DurationScope&) = delete;

 private:
  using StopWatch = folly::stop_watch<>;
  StopWatch stopWatch_;
  StatsPtr stats_;
  std::function<void(Stats& stats, StopWatch::duration)> updateScope_;
};

} // namespace facebook::eden
