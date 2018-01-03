/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/ThreadLocal.h>
#include <folly/futures/Future.h>
#include <folly/io/async/Request.h>
#include <sys/stat.h>
#include "eden/fs/fuse/EdenStats.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/FuseTypes.h"

namespace facebook {
namespace eden {
namespace fusell {

class Dispatcher;

class RequestData : public folly::RequestData {
  FuseChannel* channel_;
  fuse_in_header fuseHeader_;
  // We're managed by this context, so we only keep a weak ref
  std::weak_ptr<folly::RequestContext> requestContext_;
  // Needed to track stats
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  EdenStats::HistogramPtr latencyHistogram_{nullptr};
  ThreadLocalEdenStats* stats_{nullptr};
  Dispatcher* dispatcher_{nullptr};

  fuse_in_header stealReq();

  struct Cancel {
    folly::Future<folly::Unit> fut_;
    explicit Cancel(folly::Future<folly::Unit>&& fut) : fut_(std::move(fut)) {}
  };

 public:
  static const std::string kKey;
  RequestData(const RequestData&) = delete;
  RequestData& operator=(const RequestData&) = delete;
  RequestData(RequestData&&) = default;
  RequestData& operator=(RequestData&&) = default;
  explicit RequestData(
      FuseChannel* channel,
      const fuse_in_header& fuseHeader,
      Dispatcher* dispatcher);
  ~RequestData();
  static RequestData& get();
  static RequestData& create(
      FuseChannel* channel,
      const fuse_in_header& fuseHeader,
      Dispatcher* dispatcher);

  bool hasCallback() override {
    return false;
  }

  // Returns true if the current context is being called from inside
  // a FUSE request, false otherwise.
  static bool isFuseRequest();

  folly::Future<folly::Unit> startRequest(
      ThreadLocalEdenStats* stats,
      EdenStats::HistogramPtr histogram);
  void finishRequest();

  // Returns the associated dispatcher instance
  Dispatcher* getDispatcher() const;

  // Returns the underlying fuse request, throwing an error if it has
  // already been released
  const fuse_in_header& getReq() const;

  // Check whether the request has already been interrupted
  bool wasInterrupted() const;

  /** Register the future chain associated with this request so that
   * we can cancel it when we receive an interrupt.
   * This function will append error handling to the future chain by
   * passing it to catchErrors() prior to registering the cancellation
   * handler.
   */
  template <typename FUTURE>
  void setRequestFuture(FUTURE&& fut) {
    this->interrupter_ =
        std::make_unique<Cancel>(this->catchErrors(std::move(fut)));
  }

  /** Append error handling clauses to a future chain
   * These clauses result in reporting a fuse request error back to the
   * kernel. */
  template <typename FUTURE>
  folly::Future<folly::Unit> catchErrors(FUTURE&& fut) {
    return fut.onError(systemErrorHandler)
        .onError(genericErrorHandler)
        .ensure([] { RequestData::get().finishRequest(); });
  }

  static void systemErrorHandler(const std::system_error& err);
  static void genericErrorHandler(const std::exception& err);

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

 private:
  std::unique_ptr<Cancel> interrupter_;
};
} // namespace fusell
} // namespace eden
} // namespace facebook
