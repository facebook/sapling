/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/RequestContext.h"

#include <folly/logging/xlog.h>

#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/SystemError.h"

using namespace std::chrono;

namespace facebook::eden {

void RequestContext::startRequest(
    EdenStats* stats,
    FsChannelThreadStats::DurationPtr stat,
    std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>&
        requestWatches) {
  startTime_ = steady_clock::now();
  XDCHECK(latencyStat_ == nullptr);
  latencyStat_ = stat;
  stats_ = stats;
  channelThreadLocalStats_ = requestWatches;
  if (channelThreadLocalStats_) {
    requestMetricsScope_ = RequestMetricsScope(channelThreadLocalStats_.get());
  }
}

void RequestContext::finishRequest() noexcept {
  try {
    const auto now = steady_clock::now();

    const auto diff = now - startTime_;
    const auto diff_ns = duration_cast<nanoseconds>(diff);

    if (stats_ != nullptr) {
      (stats_->getFsChannelStatsForCurrentThread().*latencyStat_)
          .addDuration(diff);
      latencyStat_ = nullptr;
      stats_ = nullptr;
    }

    if (channelThreadLocalStats_) {
      { auto temp = std::move(requestMetricsScope_); }
      channelThreadLocalStats_.reset();
    }

    if (auto pid = getClientPid(); pid.has_value()) {
      switch (getEdenTopStats().getFetchOrigin()) {
        case Origin::FromMemoryCache:
          pal_.recordAccess(
              *pid, ProcessAccessLog::AccessType::FsChannelMemoryCacheImport);
          break;
        case Origin::FromDiskCache:
          pal_.recordAccess(
              *pid, ProcessAccessLog::AccessType::FsChannelDiskCacheImport);
          break;
        case Origin::FromNetworkFetch:
          pal_.recordAccess(
              *pid, ProcessAccessLog::AccessType::FsChannelBackingStoreImport);
          break;
        default:
          break;
      }
      pal_.recordDuration(*pid, diff_ns);
    }
  } catch (const std::exception& ex) {
    XLOG(WARN) << "Failed to complete request: " << folly::exceptionStr(ex);
  }
}

} // namespace facebook::eden
