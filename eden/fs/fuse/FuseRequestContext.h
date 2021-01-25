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
#include "eden/fs/inodes/RequestContext.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"

namespace facebook {
namespace eden {

/**
 * Each FUSE request has a corresponding FuseRequestContext object that is
 * allocated at request start and deallocated when it finishes.
 *
 * Unless a member function indicates otherwise, FuseRequestContext may be used
 * from multiple threads, but only by one thread at a time.
 */
class FuseRequestContext : public RequestContext {
 public:
  FuseRequestContext(const FuseRequestContext&) = delete;
  FuseRequestContext& operator=(const FuseRequestContext&) = delete;
  FuseRequestContext(FuseRequestContext&&) = delete;
  FuseRequestContext& operator=(FuseRequestContext&&) = delete;
  explicit FuseRequestContext(
      FuseChannel* channel,
      const fuse_in_header& fuseHeader);

  // Override of `ObjectFetchContext`
  std::optional<pid_t> getClientPid() const override {
    return static_cast<pid_t>(fuseHeader_.pid);
  }

  std::optional<int32_t> getResult() const {
    return error_;
  }

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
    error_ = 0;
    channel_->sendReply(stealReq(), payload);
  }

  template <typename T>
  void sendReply(T&& payload) {
    error_ = 0;
    channel_->sendReply(stealReq(), std::forward<T>(payload));
  }

  // Reply with a negative errno value or 0 for success
  void replyError(int err);

  // Don't send a reply, just release req_
  void replyNone();

 private:
  fuse_in_header stealReq();

  FuseChannel* channel_;
  fuse_in_header fuseHeader_;

  std::optional<int32_t> error_;
};

} // namespace eden
} // namespace facebook
