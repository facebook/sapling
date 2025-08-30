/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>
#include <atomic>
#include <utility>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/telemetry/StructuredLogger.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/ProcessAccessLog.h"

namespace facebook::eden {

class FsObjectFetchContext : public ObjectFetchContext {
 public:
  struct EdenTopStats {
   public:
    Origin getFetchOrigin() {
      return fetchOrigin_.load(std::memory_order_relaxed);
    }

    void setFetchOrigin(Origin origin) {
      fetchOrigin_.store(origin, std::memory_order_relaxed);
    }

    std::chrono::nanoseconds fuseDuration{0};

   private:
    std::atomic<Origin> fetchOrigin_{Origin::NotFetched};
  };

  EdenTopStats& getEdenTopStats() {
    return edenTopStats_;
  }

  // ObjectFetchContext overrides:

  void didFetch(ObjectType /*type*/, const ObjectId& /*id*/, Origin origin)
      override {
    edenTopStats_.setFetchOrigin(origin);
  }

  Cause getCause() const override {
    return Cause::Fs;
  }

  ImportPriority getPriority() const override {
    return priority_.load(std::memory_order_acquire);
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }

  void deprioritize(uint64_t delta) override {
    ImportPriority prev = priority_.load(std::memory_order_acquire);
    priority_.compare_exchange_strong(
        prev, prev.adjusted(-delta), std::memory_order_acq_rel);
    if (auto client_pid = getClientPid()) {
      XLOGF(
          DBG7,
          "priority for {} has changed to: {}",
          client_pid.value(),
          priority_.load().value());
    }
  }

 private:
  EdenTopStats edenTopStats_;

  /**
   * Normally, one requestData is created for only one fetch request,
   * so priority will only be accessed by one thread, but that is
   * not strictly guaranteed. Atomic is used here because there
   * might be rare cases where multiple threads access priority_
   * at the same time.
   */
  std::atomic<ImportPriority> priority_{kDefaultFsImportPriority};
};

using FsObjectFetchContextPtr = RefPtr<FsObjectFetchContext>;

class RequestContext {
 public:
  explicit RequestContext(
      ProcessAccessLog& pal,
      std::shared_ptr<StructuredLogger> logger,
      std::chrono::nanoseconds longRunningFsRequestThreshold,
      FsObjectFetchContextPtr fsObjectFetchContext) noexcept
      : longRunningFsRequestThreshold_{longRunningFsRequestThreshold},
        pal_{pal},
        logger_{std::move(logger)},
        fsObjectFetchContext_{std::move(fsObjectFetchContext)} {}
  ~RequestContext() noexcept;

  RequestContext(const RequestContext&) = delete;
  RequestContext& operator=(const RequestContext&) = delete;
  RequestContext(RequestContext&&) = delete;
  RequestContext& operator=(RequestContext&&) = delete;

  template <typename T>
  void startRequest(
      EdenStatsPtr stats,
      StatsGroupBase::Duration T::*duration,
      std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>
          requestWatches) {
    return startRequest(
        std::move(stats),
        [duration](EdenStats& stats) -> StatsGroupBase::Duration& {
          return stats.getStatsForCurrentThread<T>().*duration;
        },
        std::move(requestWatches));
  }

  const ObjectFetchContextPtr& getObjectFetchContext() const {
    return fsObjectFetchContext_.as<ObjectFetchContext>();
  }

  FsObjectFetchContext& getFsObjectFetchContext() const {
    return *fsObjectFetchContext_;
  }

 private:
  // RequestContext is used for every FsChannel implementation, each of which
  // has its own statistics. If non-empty, this function returns a Duration
  // object corresponding to the current request. The closure captured contains
  // a single pointer-to-member which will fit, without allocation, in
  // std::function's small buffer optimization on all mainstream standard
  // library implementations. In effect, std::function is a convenient
  // expression of an existential type.
  using DurationFn = std::function<StatsGroupBase::Duration&(EdenStats&)>;

  void startRequest(
      EdenStatsPtr stats,
      DurationFn durationFn,
      std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>
          requestWatches);

  void finishRequest() noexcept;

  void reportLongRunningRequest(const std::chrono::nanoseconds& duration);

  // Needed to track stats
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  EdenStatsPtr stats_;
  DurationFn latencyStat_;
  const std::chrono::nanoseconds longRunningFsRequestThreshold_;

  RequestMetricsScope requestMetricsScope_;
  std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>
      requestWatchList_;
  ProcessAccessLog& pal_;
  std::shared_ptr<StructuredLogger> logger_;

  const FsObjectFetchContextPtr fsObjectFetchContext_;
};

} // namespace facebook::eden
