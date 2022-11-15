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

#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/RequestContext.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/FsChannelTypes.h"

namespace facebook::eden {

class FuseObjectFetchContext : public FsObjectFetchContext {
 public:
  FuseObjectFetchContext(pid_t pid, uint32_t opcode)
      : pid_{pid}, opcode_{opcode} {}

  std::optional<pid_t> getClientPid() const override {
    return pid_;
  }

  std::optional<std::string_view> getCauseDetail() const override {
    return fuseOpcodeName(opcode_);
  }

 private:
  pid_t pid_;
  uint32_t opcode_;
};

/**
 * Each FUSE request has a corresponding FuseRequestContext object that is
 * allocated at request start and deallocated when it finishes.
 *
 * Unless a member function indicates otherwise, FuseRequestContext may be used
 * from multiple threads, but only by one thread at a time.
 */
class FuseRequestContext : public RequestContext {
 public:
  explicit FuseRequestContext(
      FuseChannel* channel,
      const fuse_in_header& fuseHeader);

  FuseRequestContext(const FuseRequestContext&) = delete;
  FuseRequestContext& operator=(const FuseRequestContext&) = delete;
  FuseRequestContext(FuseRequestContext&&) = delete;
  FuseRequestContext& operator=(FuseRequestContext&&) = delete;

  /**
   * After sendReply or replyError, this returns the error code we returned to
   * the kernel, negated.
   *
   * After sendReplyWithInode, this returns the inode number that the kernel
   * will reference until it sends FORGET.
   */
  const std::optional<int64_t>& getResult() const {
    return result_;
  }

  /**
   * Returns the underlying fuse request, throwing an error if it has
   * already been released
   */
  const fuse_in_header& getReq() const;

  /**
   * Append error handling clauses to a future chain. These clauses result in
   * reporting a fuse request error back to the kernel.
   */
  folly::Future<folly::Unit> catchErrors(
      folly::Future<folly::Unit>&& fut,
      Notifier* FOLLY_NULLABLE notifier) {
    return std::move(fut).thenTryInline([this, notifier](
                                            folly::Try<folly::Unit>&& try_) {
      if (try_.hasException()) {
        if (auto* err = try_.tryGetExceptionObject<folly::FutureTimeout>()) {
          timeoutErrorHandler(*err, notifier);
        } else if (
            auto* err = try_.tryGetExceptionObject<std::system_error>()) {
          systemErrorHandler(*err, notifier);
        } else if (auto* err = try_.tryGetExceptionObject<std::exception>()) {
          genericErrorHandler(*err, notifier);
        } else {
          genericErrorHandler(
              std::runtime_error{"unknown exception type"}, notifier);
        }
      }
    });
  }

  void systemErrorHandler(
      const std::system_error& err,
      Notifier* FOLLY_NULLABLE notifier);
  void genericErrorHandler(
      const std::exception& err,
      Notifier* FOLLY_NULLABLE notifier);
  void timeoutErrorHandler(
      const folly::FutureTimeout& err,
      Notifier* FOLLY_NULLABLE notifier);

  template <typename... T>
  void sendReply(T&&... payload) {
    channel_->sendReply(stealReqWithResult(0), std::forward<T>(payload)...);
  }

  /**
   * Same as sendReply, but is called when the kernel will take a reference to
   * the returned inode. The returned inode value will be logged to make trace
   * logs more useful.
   */
  template <typename T>
  void sendReplyWithInode(uint64_t nodeid, T&& reply) {
    channel_->sendReply(stealReqWithResult(nodeid), std::forward<T>(reply));
  }

  // Reply with a negative errno value or 0 for success
  void replyError(int err);

  // Don't send a reply, just release req_
  void replyNone();

 private:
  // Returns the header and sets result_ to indicate
  // that the request has been released.
  fuse_in_header stealReqWithResult(int64_t result);

  FuseChannel* channel_;
  const fuse_in_header fuseHeader_;

  std::optional<int64_t> result_;
};

} // namespace facebook::eden
