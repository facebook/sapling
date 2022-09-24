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

#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/ProcessAccessLog.h"

namespace facebook::eden {

class RequestContext : public ObjectFetchContext {
 public:
  explicit RequestContext(ProcessAccessLog& pal) : pal_(pal) {}

  /**
   * Allocate a RequestContext.
   *
   * Sub-classes should call this instead of std::make_shared to make sure that
   * finishRequest is called once the last shared_ptr holding the
   * RequestContext is destroyed.
   */
  template <typename T, typename... Args>
  static std::
      enable_if_t<std::is_base_of_v<RequestContext, T>, std::shared_ptr<T>>
      makeSharedRequestContext(Args&&... args) {
    return std::shared_ptr<T>{new T(std::forward<Args>(args)...), [](T* ptr) {
                                ptr->finishRequest();
                                delete ptr;
                              }};
  }

  RequestContext(const RequestContext&) = delete;
  RequestContext& operator=(const RequestContext&) = delete;
  RequestContext(RequestContext&&) = delete;
  RequestContext& operator=(RequestContext&&) = delete;

  /**
   * Override of `ObjectFetchContext`
   *
   * Unlike other RequestContext function, this may be called concurrently by
   * arbitrary threads.
   */
  void didFetch(ObjectType /*type*/, const ObjectId& /*hash*/, Origin origin)
      override {
    edenTopStats_.setFetchOrigin(origin);
  }

  // Override of `getPriority`
  ImportPriority getPriority() const override {
    return priority_;
  }

  // Override of `deprioritize`
  virtual void deprioritize(uint64_t delta) override {
    ImportPriority prev = priority_.load();
    priority_.compare_exchange_strong(prev, prev.getDeprioritized(delta));
    if (getClientPid().has_value()) {
      XLOG(DBG7) << "priority for " << getClientPid().value()
                 << " has changed to: " << priority_.load().value();
    }
  }

  // Override of `ObjectFetchContext`
  Cause getCause() const override {
    return ObjectFetchContext::Cause::Fs;
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }

  void startRequest(
      EdenStats* stats,
      FsChannelThreadStats::DurationPtr stat,
      std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>&
          requestWatches);

 private:
  void finishRequest() noexcept;

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

  // Needed to track stats
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  FsChannelThreadStats::DurationPtr latencyStat_ = nullptr;
  EdenStats* stats_ = nullptr;
  RequestMetricsScope requestMetricsScope_;
  std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>
      channelThreadLocalStats_;
  ProcessAccessLog& pal_;
  EdenTopStats edenTopStats_;

  /**
   * Normally, one requestData is created for only one fetch request,
   * so priority will only be accessed by one thread, but that is
   * not strictly guaranteed. Atomic is used here because there
   * might be rare cases where multiple threads access priority_
   * at the same time.
   */
  std::atomic<ImportPriority> priority_{
      ImportPriority(ImportPriorityKind::High)};
};

} // namespace facebook::eden
