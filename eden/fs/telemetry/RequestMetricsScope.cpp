/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "RequestMetricsScope.h"

#include <algorithm>
#include <numeric>

#include <folly/String.h>

#include "eden/fs/utils/Bug.h"

namespace facebook {
namespace eden {

RequestMetricsScope::RequestMetricsScope(
    LockedRequestWatchList* pendingRequestWatches)
    : pendingRequestWatches_(pendingRequestWatches) {
  folly::stop_watch<> watch;
  {
    auto startTimes = pendingRequestWatches_->wlock();
    requestWatch_ = startTimes->insert(startTimes->end(), watch);
  }
}

RequestMetricsScope::RequestMetricsScope() : pendingRequestWatches_(nullptr) {}

RequestMetricsScope::RequestMetricsScope(RequestMetricsScope&& other) noexcept
    : pendingRequestWatches_(std::move(other.pendingRequestWatches_)),
      requestWatch_(std::move(other.requestWatch_)) {
  other.pendingRequestWatches_ = nullptr;
}

RequestMetricsScope& RequestMetricsScope::operator=(
    RequestMetricsScope&& other) {
  this->pendingRequestWatches_ = std::move(other.pendingRequestWatches_);
  this->requestWatch_ = std::move(other.requestWatch_);
  other.pendingRequestWatches_ = nullptr;
  return *this;
}

RequestMetricsScope::~RequestMetricsScope() {
  if (pendingRequestWatches_ != nullptr) {
    auto startTimes = pendingRequestWatches_->wlock();
    startTimes->erase(requestWatch_);
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
  EDEN_BUG() << "unknown metric " << static_cast<int>(metric);
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
  EDEN_BUG() << "unknown metric " << static_cast<int>(metric);
}

RequestMetricsScope::DefaultRequestDuration RequestMetricsScope::getMaxDuration(
    const LockedRequestWatchList& watches) {
  DefaultRequestDuration maxDurationImport{0};
  {
    auto lockedWatches = watches.rlock();
    for (const auto& watch : *lockedWatches) {
      maxDurationImport = std::max(maxDurationImport, watch.elapsed());
    }
  }
  return maxDurationImport;
}

} // namespace eden
} // namespace facebook
