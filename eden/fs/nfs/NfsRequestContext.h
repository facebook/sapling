/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/RequestContext.h"

namespace facebook::eden {

class NfsRequestContext : public RequestContext {
 public:
  /**
   * Constructs a new NfsRequestContext. The context should live for the
   * duration of the NFS request.
   * `startRequest` should be called at the beginning and `finishRequest` at the
   * end of the request. The `causeDetail` is copied as is and thus the lifetime
   * of the underlying string must exceed the lifetime of the NfsRequestContext.
   * The caller is responsible for ensuring this.
   */
  explicit NfsRequestContext(
      uint32_t xid,
      folly::StringPiece causeDetail,
      ProcessAccessLog& processAccessLog);

  NfsRequestContext(const NfsRequestContext&) = delete;
  NfsRequestContext& operator=(const NfsRequestContext&) = delete;
  NfsRequestContext(NfsRequestContext&&) = delete;
  NfsRequestContext& operator=(NfsRequestContext&&) = delete;

  std::optional<folly::StringPiece> getCauseDetail() const override {
    return std::make_optional(causeDetail_);
  }

  inline uint32_t getXid() const {
    return xid_;
  }

 private:
  uint32_t xid_;
  folly::StringPiece causeDetail_;
};

} // namespace facebook::eden
