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
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/tracing/EdenStats.h"

namespace facebook {
namespace eden {

class Dispatcher;

class RequestData : public folly::RequestData {
  FuseChannel* channel_;
  fuse_in_header fuseHeader_;
  // Needed to track stats
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  FuseThreadStats::HistogramPtr latencyHistogram_{nullptr};
  EdenStats* stats_{nullptr};
  Dispatcher* dispatcher_{nullptr};

  fuse_in_header stealReq();

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

  // Returns true if the current context is being called from inside
  // a FUSE request, false otherwise.
  static bool isFuseRequest();

  void startRequest(EdenStats* stats, FuseThreadStats::HistogramPtr histogram);
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

  /** Append error handling clauses to a future chain
   * These clauses result in reporting a fuse request error back to the
   * kernel. */
  template <typename T>
  folly::Future<folly::Unit> catchErrors(folly::Future<T>&& fut) {
    return std::move(fut)
        .thenError(folly::tag_t<std::system_error>{}, systemErrorHandler)
        .thenError(folly::tag_t<std::exception>{}, genericErrorHandler)
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
};

} // namespace eden
} // namespace facebook
