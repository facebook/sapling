/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/RequestContext.h"

#include <folly/logging/xlog.h>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/utils/SystemError.h"

using namespace std::chrono;

namespace facebook::eden {

RequestContext::~RequestContext() noexcept {
  try {
    const auto diff = steady_clock::now() - startTime_;
    const auto diff_ns = duration_cast<nanoseconds>(diff);

    if (stats_) {
      XCHECK(latencyStat_) << "stats_ and latencyStat_ must be set together";
      latencyStat_(*stats_).addDuration(diff);
    }

    if (requestWatchList_) {
      {
        auto temp = std::move(requestMetricsScope_);
      }
      requestWatchList_.reset();
    }

    if (auto pid = fsObjectFetchContext_->getClientPid()) {
      switch (fsObjectFetchContext_->getEdenTopStats().getFetchOrigin()) {
        case ObjectFetchContext::Origin::FromMemoryCache:
          pal_.recordAccess(
              pid.value().get(),
              ProcessAccessLog::AccessType::FsChannelMemoryCacheImport);
          break;
        case ObjectFetchContext::Origin::FromDiskCache:
          pal_.recordAccess(
              pid.value().get(),
              ProcessAccessLog::AccessType::FsChannelDiskCacheImport);
          break;
        case ObjectFetchContext::Origin::FromNetworkFetch:
          pal_.recordAccess(
              pid.value().get(),
              ProcessAccessLog::AccessType::FsChannelBackingStoreImport);
          break;
        default:
          break;
      }
      pal_.recordDuration(pid.value().get(), diff_ns);
    }
  } catch (const std::exception& ex) {
    XLOG(WARN) << "Failed to complete request: " << folly::exceptionStr(ex);
  }
}

void RequestContext::startRequest(
    EdenStatsPtr stats,
    DurationFn stat,
    std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>
        requestWatches) {
  startTime_ = steady_clock::now();
  XDCHECK(!latencyStat_);
  stats_ = std::move(stats);
  latencyStat_ = std::move(stat);
  requestWatchList_ = requestWatches;
  if (requestWatchList_) {
    requestMetricsScope_ = RequestMetricsScope(requestWatchList_.get());
  }
}

} // namespace facebook::eden
