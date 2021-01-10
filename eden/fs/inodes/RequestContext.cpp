/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/RequestContext.h"

#include <folly/logging/xlog.h>

#include "eden/fs/notifications/Notifications.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/SystemError.h"

using namespace std::chrono;

namespace facebook::eden {

void RequestContext::startRequest(
    EdenStats* stats,
    ChannelThreadStats::HistogramPtr histogram,
    std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>&
        requestWatches) {
  startTime_ = steady_clock::now();
  XDCHECK(latencyHistogram_ == nullptr);
  latencyHistogram_ = histogram;
  stats_ = stats;
  channelThreadLocalStats_ = requestWatches;
  if (channelThreadLocalStats_) {
    requestMetricsScope_ = RequestMetricsScope(channelThreadLocalStats_.get());
  }
}

void RequestContext::finishRequest() {
  const auto now = steady_clock::now();

  const auto diff = now - startTime_;
  const auto diff_us = duration_cast<microseconds>(diff);
  const auto diff_ns = duration_cast<nanoseconds>(diff);

  stats_->getChannelStatsForCurrentThread().recordLatency(
      latencyHistogram_, diff_us);
  latencyHistogram_ = nullptr;
  stats_ = nullptr;

  if (channelThreadLocalStats_) {
    { auto temp = std::move(requestMetricsScope_); }
    channelThreadLocalStats_.reset();
  }

  if (auto pid = getClientPid(); pid.has_value()) {
    if (getEdenTopStats().didImportFromBackingStore()) {
      auto type = ProcessAccessLog::AccessType::FsChannelBackingStoreImport;
      pal_.recordAccess(*pid, type);
    }
    pal_.recordDuration(*pid, diff_ns);
  }
}

} // namespace facebook::eden
