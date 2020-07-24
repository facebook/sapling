/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>
#include <folly/io/async/Request.h>
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

class Dispatcher;

/**
 * Follows a request across executors and futures. startRequest should be
 * called before initiating a request and will be run where this is
 * initated. catchErrors will wrap a request future so that finishRequest is
 * called when it is completed. finishRequest will be executed where the request
 * is executed which may be in a different thread than startRequest was run.
 * Thus this will be run single threaded (only running from one thread at a
 * time, but may run on different threads.
 *
 * see folly/io/async/Request.h for more info on RequestData
 * see eden/fs/fuse/FuseChannel.cpp FuseChannel::processSession for how this
 * should be used
 */
class RequestData : public folly::RequestData, public ObjectFetchContext {
  FuseChannel* channel_;
  fuse_in_header fuseHeader_;
  // Needed to track stats
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  FuseThreadStats::HistogramPtr latencyHistogram_{nullptr};
  EdenStats* stats_{nullptr};
  Dispatcher* dispatcher_{nullptr};
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
  static const std::string kKey;
  RequestData(const RequestData&) = delete;
  RequestData& operator=(const RequestData&) = delete;
  RequestData(RequestData&&) = delete;
  RequestData& operator=(RequestData&&) = delete;
  explicit RequestData(
      FuseChannel* channel,
      const fuse_in_header& fuseHeader,
      Dispatcher* dispatcher);
  static RequestData& get();
  static RequestData& create(
      FuseChannel* channel,
      const fuse_in_header& fuseHeader,
      Dispatcher* dispatcher);

  bool hasCallback() override {
    return false;
  }

  // Override of `ObjectFetchContext`
  void didFetch(ObjectType /*type*/, const Hash& /*hash*/, Origin origin)
      override {
    if (origin == Origin::FromBackingStore) {
      edenTopStats_.setDidImportFromBackingStore();
    }
  }

  // Override of `ObjectFetchContext`
  std::optional<pid_t> getClientPid() const override {
    if (fuseHeader_.opcode == 0) {
      return std::nullopt;
    }
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
    return ObjectFetchContext::Cause::Fuse;
  }

  // Returns true if the current context is being called from inside
  // a FUSE request, false otherwise.
  static bool isFuseRequest();

  void startRequest(
      EdenStats* stats,
      FuseThreadStats::HistogramPtr histogram,
      std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>&
          requestWatches);
  void finishRequest();

  // Returns the associated dispatcher instance
  Dispatcher* getDispatcher() const;

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
  template <typename T>
  folly::Future<folly::Unit> catchErrors(
      folly::Future<T>&& fut,
      Notifications* FOLLY_NULLABLE notifications) {
    return std::move(fut)
        .thenError(
            folly::tag_t<folly::FutureTimeout>{},
            [notifications](auto&& err) {
              timeoutErrorHandler(err, notifications);
            })
        .thenError(
            folly::tag_t<std::system_error>{},
            [notifications](auto&& err) {
              systemErrorHandler(err, notifications);
            })
        .thenError(
            folly::tag_t<std::exception>{},
            [notifications](auto&& err) {
              genericErrorHandler(err, notifications);
            })
        .ensure([] { RequestData::get().finishRequest(); });
  }

  static void systemErrorHandler(
      const std::system_error& err,
      Notifications* FOLLY_NULLABLE notifications);
  static void genericErrorHandler(
      const std::exception& err,
      Notifications* FOLLY_NULLABLE notifications);
  static void timeoutErrorHandler(
      const folly::FutureTimeout& err,
      Notifications* FOLLY_NULLABLE notifications);

  template <typename T>
  void sendReply(const T& payload) {
    channel_->sendReply(stealReq(), payload);
  }

  void sendReply(folly::ByteRange bytes) {
    channel_->sendReply(stealReq(), bytes);
  }

  void sendReply(folly::fbvector<iovec>&& vec) {
    channel_->sendReply(stealReq(), std::move(vec));
  }

  void sendReply(folly::StringPiece piece) {
    channel_->sendReply(stealReq(), folly::ByteRange(piece));
  }

  // Reply with a negative errno value or 0 for success
  void replyError(int err);

  // Don't send a reply, just release req_
  void replyNone();
};

} // namespace eden
} // namespace facebook
