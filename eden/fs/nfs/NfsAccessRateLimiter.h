/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/TokenBucket.h>

namespace facebook::eden {

/**
 * Token-bucket rate limiter for the NFS "rate_limit" access mode: allows a
 * sustained `count` accesses per `windowSeconds` (refilling continuously)
 * with bursts of up to `count`. Wraps folly::DynamicTokenBucket so the
 * count/window parameters can change on every call — they are re-read from
 * ReloadableConfig per request — and stays lock-free for concurrent RPC
 * threads.
 */
class NfsAccessRateLimiter {
 public:
  /**
   * Records one access and returns true when it is within the allowed
   * rate. `nowSeconds` is injectable for tests; it defaults to folly's
   * steady clock and must be monotonically non-decreasing across calls.
   *
   * Degenerate configs: a zero count admits nothing (every access is over
   * the limit); a zero window admits everything (an infinite refill rate).
   */
  bool allow(
      uint64_t count,
      uint64_t windowSeconds,
      double nowSeconds = folly::DynamicTokenBucket::defaultClockNow()) const {
    if (count == 0) {
      return false;
    }
    if (windowSeconds == 0) {
      return true;
    }
    return bucket_.consume(
        1.0,
        static_cast<double>(count) / static_cast<double>(windowSeconds),
        static_cast<double>(count),
        nowSeconds);
  }

 private:
  // The bucket is internally atomic, so sharing it under a read lock is safe.
  mutable folly::DynamicTokenBucket bucket_;
};

} // namespace facebook::eden
