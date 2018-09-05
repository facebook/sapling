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
#include <folly/futures/Future.h>
#include <folly/io/async/Request.h>
#include <atomic>
#include <utility>
#include "eden/fs/fuse/EdenStats.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/FuseTypes.h"

namespace facebook {
namespace eden {

class Dispatcher;

class RequestData : public folly::RequestData {
  FuseChannel* channel_;
  fuse_in_header fuseHeader_;
  // Needed to track stats
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  EdenStats::HistogramPtr latencyHistogram_{nullptr};
  ThreadLocalEdenStats* stats_{nullptr};
  Dispatcher* dispatcher_{nullptr};

  fuse_in_header stealReq();

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

  void startRequest(
      ThreadLocalEdenStats* stats,
      EdenStats::HistogramPtr histogram);
  void finishRequest();

  // Returns the associated dispatcher instance
  Dispatcher* getDispatcher() const;

  // Returns the underlying fuse request, throwing an error if it has
  // already been released
  const fuse_in_header& getReq() const;

  // Returns the underlying fuse request. Unlike getReq this function doesn't
  // throw. The caller is responsible to verify that the fuse_in_header is valid
  // by checking if (fuseHeader.opcode != 0)
  const fuse_in_header& examineReq() const;

  /** Register the future chain associated with this request so that
   * we can cancel it when we receive an interrupt.
   * This function will append error handling to the future chain by
   * passing it to catchErrors() prior to registering the cancellation
   * handler.
   */
  template <typename FUTURE>
  void setRequestFuture(FUTURE&& fut) {
    this->interrupter_ = this->catchErrors(std::forward<FUTURE>(fut));

    // Flag that the interrupter_ member has been initialised and that
    // the operation can now be interrupted. If the previous value of the
    // flag indicates that an interrupt was requested concurrently with this
    // operation before we could finish launching it then we should interrupt
    // it now.
    auto oldValue = interruptFlag_.fetch_or(
        kInterrupterInitialisedFlag, std::memory_order_acq_rel);
    CHECK_EQ(0, oldValue & kInterrupterInitialisedFlag);
    if (oldValue & kInterruptRequestedFlag) {
      this->interrupter_.cancel();
    }
  }

  /** Append error handling clauses to a future chain
   * These clauses result in reporting a fuse request error back to the
   * kernel. */
  template <typename T>
  folly::Future<folly::Unit> catchErrors(folly::Future<T>&& fut) {
    return std::move(fut)
        .onError(systemErrorHandler)
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

  // Notify this request about EINTR.  This causes the future to be
  // cancel()'d and may result in it stopping what it was doing
  // before it is complete.
  void interrupt();

 private:
  folly::Future<folly::Unit> interrupter_;

  // This atomic variable is a set of two flags is used to decide the race
  // between the thread calling setRequestFuture() to register the future that
  // will complete when the request completes, and another thread concurrently
  // calling interrupt() when handling a FUSE_INTERRUPT request that comes in
  // before we finish launching the corresponding request.
  //
  // bit 0 - set if interrupter_ has been initialised.
  // bit 1 - set if a request has been made to interrupt the operation.
  std::atomic<std::uint8_t> interruptFlag_{0};

  static constexpr std::uint8_t kInterrupterInitialisedFlag = 1;
  static constexpr std::uint8_t kInterruptRequestedFlag = 2;
};

} // namespace eden
} // namespace facebook
