/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <list>
#include <memory>
#include <string>

#include <folly/String.h>
#include <folly/Synchronized.h>
#include <folly/stop_watch.h>

namespace facebook {
namespace eden {

/**
 * Represents a request tracked in a RequestMetricsScope::RequestWatchList.
 * To track a request a RequestMetricsScope object should be in scope for the
 * duration of the request.
 *
 * The scope inserts a watch into the given list on construction and removes
 * that watch on destruction.
 */
class RequestMetricsScope {
 public:
  using RequestWatchList = std::list<folly::stop_watch<>>;
  using LockedRequestWatchList = folly::Synchronized<RequestWatchList>;
  using DefaultRequestDuration =
      std::chrono::steady_clock::steady_clock::duration;

  RequestMetricsScope(LockedRequestWatchList* pendingRequestWatches);
  RequestMetricsScope();
  RequestMetricsScope(RequestMetricsScope&&) noexcept;
  RequestMetricsScope& operator=(RequestMetricsScope&&);
  RequestMetricsScope(const RequestMetricsScope&) = delete;
  RequestMetricsScope& operator=(const RequestMetricsScope&) = delete;

  ~RequestMetricsScope();

  /**
   * Metrics to calculated for any type of request tracked with
   * RequestMetricsScope
   */
  enum RequestMetric {
    // number of requests
    COUNT,
    // duration of the longest current import
    MAX_DURATION_US,
  };

  constexpr static std::array<RequestMetric, 2> requestMetrics{
      RequestMetric::COUNT,
      RequestMetric::MAX_DURATION_US};

  static folly::StringPiece stringOfRequestMetric(RequestMetric metric);

  /**
   * combine the values of the counters in a way that makes sense
   * for the `metric` being calculated
   */
  static size_t aggregateMetricCounters(
      RequestMetricsScope::RequestMetric metric,
      std::vector<size_t>& counters);

  /**
   * calculates the `metric` from the `watches` which track
   * the duration of all of a certain type of request
   */
  static size_t getMetricFromWatches(
      RequestMetric metric,
      const LockedRequestWatchList& watches);

  /**
   * finds the watch in `watches` for which the time that has elapsed
   * is the greatest and returns the duration of time that has elapsed
   */
  static DefaultRequestDuration getMaxDuration(
      const LockedRequestWatchList& watches);

 private:
  LockedRequestWatchList* pendingRequestWatches_;
  RequestWatchList::iterator requestWatch_;
}; // namespace eden
} // namespace eden
} // namespace facebook
