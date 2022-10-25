/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "RequestMetricsScope.h"

#include <algorithm>
#include <numeric>

#include <folly/String.h>

#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/EnumValue.h"

namespace facebook::eden {

RequestMetricsScope::RequestMetricsScope() : pendingRequestWatches_{nullptr} {}

RequestMetricsScope::RequestMetricsScope(
    LockedRequestWatchList* pendingRequestWatches)
    : pendingRequestWatches_{pendingRequestWatches} {
  folly::stop_watch<> watch;
  {
    auto startTimes = pendingRequestWatches_->wlock();
    requestWatch_ = startTimes->insert(startTimes->end(), watch);
  }
}

RequestMetricsScope::RequestMetricsScope(RequestMetricsScope&& that) noexcept
    : pendingRequestWatches_{std::exchange(
          that.pendingRequestWatches_,
          nullptr)},
      requestWatch_{that.requestWatch_} {}

RequestMetricsScope& RequestMetricsScope::operator=(
    RequestMetricsScope&& that) noexcept {
  pendingRequestWatches_ = std::exchange(that.pendingRequestWatches_, nullptr);
  requestWatch_ = that.requestWatch_;
  return *this;
}

RequestMetricsScope::~RequestMetricsScope() {
  if (pendingRequestWatches_) {
    auto startTimes = pendingRequestWatches_->wlock();
    startTimes->erase(requestWatch_);
  }
}

void RequestMetricsScope::reset() {
  if (pendingRequestWatches_) {
    auto startTimes = pendingRequestWatches_->wlock();
    startTimes->erase(requestWatch_);
    pendingRequestWatches_ = nullptr;
  }
}

folly::StringPiece RequestMetricsScope::stringOfRequestMetric(
    RequestMetric metric) {
  switch (metric) {
    case RequestMetric::COUNT:
      return "count";
    case RequestMetric::MAX_DURATION_US:
      return "max_duration_us";
  }
  EDEN_BUG() << "unknown metric " << enumValue(metric);
}

folly::StringPiece RequestMetricsScope::stringOfHgImportStage(
    RequestStage stage) {
  switch (stage) {
    case RequestStage::PENDING:
      return "pending_import";
    case RequestStage::LIVE:
      return "live_import";
  }
  EDEN_BUG() << "unknown hg import stage " << static_cast<int>(stage);
}

folly::StringPiece RequestMetricsScope::stringOfFuseRequestStage(
    RequestStage stage) {
  switch (stage) {
    case RequestStage::PENDING:
      return "pending_requests";
    case RequestStage::LIVE:
      return "live_requests";
  }
  EDEN_BUG() << "unknown hg import stage " << static_cast<int>(stage);
}

size_t RequestMetricsScope::aggregateMetricCounters(
    RequestMetricsScope::RequestMetric metric,
    std::vector<size_t>& counters) {
  switch (metric) {
    case RequestMetricsScope::RequestMetric::COUNT:
      return std::accumulate(counters.begin(), counters.end(), size_t{0});
    case RequestMetricsScope::RequestMetric::MAX_DURATION_US:
      auto max = std::max_element(counters.begin(), counters.end());
      return max == counters.end() ? size_t{0} : *max;
  }
  EDEN_BUG() << "unknown request metric type " << static_cast<int>(metric);
}

size_t RequestMetricsScope::getMetricFromWatches(
    RequestMetric metric,
    const LockedRequestWatchList& watches) {
  switch (metric) {
    case COUNT:
      return watches.rlock()->size();
    case MAX_DURATION_US:
      return static_cast<size_t>(
          std::chrono::duration_cast<std::chrono::microseconds>(
              getMaxDuration(watches))
              .count());
  }
  EDEN_BUG() << "unknown metric " << enumValue(metric);
}

RequestMetricsScope::DefaultRequestDuration RequestMetricsScope::getMaxDuration(
    const LockedRequestWatchList& watches) {
  {
    auto lockedWatches = watches.rlock();
    if (lockedWatches->empty()) {
      return RequestMetricsScope::DefaultRequestDuration{0};
    }

    // By virtue of enqueing new watches at the end of the list, the front will
    // always be the watch that has been in the list the longest, ie: the one
    // with the max duration.
    return lockedWatches->front().elapsed();
  }
}

} // namespace facebook::eden
