/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>
#include <atomic>
#include <utility>

#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"

namespace facebook {
namespace eden {

/**
 * Each FUSE request has a corresponding RequestData object that is allocated at
 * request start and deallocated when it finishes.
 *
 * Unless a member function indicates otherwise, RequestData may be used from
 * multiple threads, but only by one thread at a time.
 */
class RequestData : public ObjectFetchContext {
  FuseChannel* channel_;
  fuse_in_header fuseHeader_;
  // Needed to track stats
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  FuseThreadStats::HistogramPtr latencyHistogram_{nullptr};
  EdenStats* stats_{nullptr};
  RequestMetricsScope requestMetricsScope_;
  std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>
      channelThreadLocalStats_;

  struct EdenTopStats {
   public:
    bool didImportFromBackingStore() const {
      return didImportFromBackingStore_.load(std::memory_order_relaxed);
    }
    void setDidImportFromBackingStore() {
      didImportFromBackingStore_.store(true, std::memory_order_relaxed);
    }
    std::chrono::nanoseconds fuseDuration{0};

   private:
    std::atomic<bool> didImportFromBackingStore_{false};
  } edenTopStats_;

  fuse_in_header stealReq();

  /**
   * Normally, one requestData is created for only one fetch request,
   * so priority will only be accessed by one thread, but that is
   * not strictly guaranteed. Atomic is used here because there
   * might be rare cases where multiple threads access priority_
   * at the same time.
   */
  std::atomic<ImportPriority> priority_{
      ImportPriority(ImportPriorityKind::High)};

 public:
  RequestData(const RequestData&) = delete;
  RequestData& operator=(const RequestData&) = delete;
  RequestData(RequestData&&) = delete;
  RequestData& operator=(RequestData&&) = delete;
  explicit RequestData(FuseChannel* channel, const fuse_in_header& fuseHeader);

  /**
   * Override of `ObjectFetchContext`
   *
   * Unlike other RequestData function, this may be called concurrently by
   * arbitrary threads.
   */
  void didFetch(ObjectType /*type*/, const Hash& /*hash*/, Origin origin)
      override {
    if (origin == Origin::FromBackingStore) {
      edenTopStats_.setDidImportFromBackingStore();
    }
  }

  // Override of `ObjectFetchContext`
  std::optional<pid_t> getClientPid() const override {
    return static_cast<pid_t>(fuseHeader_.pid);
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
    return ObjectFetchContext::Cause::Channel;
  }

  void startRequest(
      EdenStats* stats,
      FuseThreadStats::HistogramPtr histogram,
      std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>&
          requestWatches);
  void finishRequest();

  EdenTopStats& getEdenTopStats();

  // Returns the underlying fuse request, throwing an error if it has
  // already been released
  const fuse_in_header& getReq() const;

  // Returns the underlying fuse request. Unlike getReq this function doesn't
  // throw. The caller is responsible to verify that the fuse_in_header is
  // valid by checking if (fuseHeader.opcode != 0)
  const fuse_in_header& examineReq() const;

  /** Append error handling clauses to a future chain
   * These clauses result in reporting a fuse request error back to the
   * kernel. */
  folly::Future<folly::Unit> catchErrors(
      folly::Future<folly::Unit>&& fut,
      Notifications* FOLLY_NULLABLE notifications) {
    return std::move(fut).thenTryInline([this, notifications](
                                            folly::Try<folly::Unit>&& try_) {
      SCOPE_EXIT {
        finishRequest();
      };
      if (try_.hasException()) {
        if (auto* err = try_.tryGetExceptionObject<folly::FutureTimeout>()) {
          timeoutErrorHandler(*err, notifications);
        } else if (
            auto* err = try_.tryGetExceptionObject<std::system_error>()) {
          systemErrorHandler(*err, notifications);
        } else if (auto* err = try_.tryGetExceptionObject<std::exception>()) {
          genericErrorHandler(*err, notifications);
        } else {
          genericErrorHandler(
              std::runtime_error{"unknown exception type"}, notifications);
        }
      }
    });
  }

  void systemErrorHandler(
      const std::system_error& err,
      Notifications* FOLLY_NULLABLE notifications);
  void genericErrorHandler(
      const std::exception& err,
      Notifications* FOLLY_NULLABLE notifications);
  void timeoutErrorHandler(
      const folly::FutureTimeout& err,
      Notifications* FOLLY_NULLABLE notifications);

  template <typename T>
  void sendReply(const T& payload) {
    channel_->sendReply(stealReq(), payload);
  }

  template <typename T>
  void sendReply(T&& payload) {
    channel_->sendReply(stealReq(), std::forward<T>(payload));
  }

  // Reply with a negative errno value or 0 for success
  void replyError(int err);

  // Don't send a reply, just release req_
  void replyNone();
};

} // namespace eden
} // namespace facebook
