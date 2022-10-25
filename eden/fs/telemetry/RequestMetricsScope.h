/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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

namespace facebook::eden {

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

  RequestMetricsScope();
  explicit RequestMetricsScope(LockedRequestWatchList* pendingRequestWatches);
  RequestMetricsScope(const RequestMetricsScope&) = delete;
  RequestMetricsScope& operator=(const RequestMetricsScope&) = delete;
  RequestMetricsScope(RequestMetricsScope&&) noexcept;
  RequestMetricsScope& operator=(RequestMetricsScope&&) noexcept;

  ~RequestMetricsScope();

  void reset();

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
   * stages of requests that are tracked, these represent where an request is in
   * the process (for example an request could be queued or live)
   */
  enum RequestStage {
    // represents any request that has been requested but not yet completed
    // (request in this stage could be in the queue, live, or in the case of
    // hg store imports fetching from cache
    PENDING,
    // represents request that are currently being executed (in the case of
    // hg imports, only those fetching data, this does not include those reading
    // from cache)
    LIVE,
  };

  constexpr static std::array<RequestStage, 2> requestStages{
      RequestStage::PENDING,
      RequestStage::LIVE};

  static folly::StringPiece stringOfHgImportStage(RequestStage stage);
  static folly::StringPiece stringOfFuseRequestStage(RequestStage stage);

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
} // namespace facebook::eden
