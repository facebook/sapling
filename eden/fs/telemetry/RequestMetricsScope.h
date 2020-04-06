/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <list>
#include <memory>

#include <folly/Synchronized.h>
#include <folly/stop_watch.h>

namespace facebook {
namespace eden {

/**
 * manages the pointer to metrics for Imports
 */
class RequestMetricsScope {
 public:
  using RequestWatchList = std::list<folly::stop_watch<>>;
  using LockedRequestWatchList = folly::Synchronized<RequestWatchList>;
  using DefaultRequestDuration =
      std::chrono::steady_clock::steady_clock::duration;

  RequestMetricsScope(LockedRequestWatchList* pendingRequestWatches)
      : pendingRequestWatches_(pendingRequestWatches) {
    folly::stop_watch<> watch;
    {
      auto startTimes = pendingRequestWatches_->wlock();
      requestWatch_ = startTimes->insert(startTimes->end(), watch);
    }
  }

  ~RequestMetricsScope() {
    {
      auto startTimes = pendingRequestWatches_->wlock();
      startTimes->erase(requestWatch_);
    }
  }

  RequestMetricsScope(RequestMetricsScope&&) = delete;
  RequestMetricsScope& operator=(RequestMetricsScope&&) = delete;

  /**
   * finds the watch in `watches` for which the time that has elapsed
   * is the greatest and returns the duration of time that has elapsed
   */
  static DefaultRequestDuration getMaxDuration(
      LockedRequestWatchList& watches) {
    DefaultRequestDuration maxDurationImport{0};
    {
      auto lockedWatches = watches.rlock();
      for (const auto& watch : *lockedWatches) {
        maxDurationImport = std::max(maxDurationImport, watch.elapsed());
      }
    }
    return maxDurationImport;
  }

 private:
  LockedRequestWatchList* pendingRequestWatches_;
  RequestWatchList::iterator requestWatch_;
};

} // namespace eden
} // namespace facebook
