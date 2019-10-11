/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/logging/xlog.h>
#include <array>

namespace facebook {
namespace eden {

/**
 * Maintains a circular buffer of `Size` `Bucket`s, each of which can accumulate
 * samples. When the clock advances, old buckets are cleared.
 *
 * Bucket must be a struct or class with an add() method, a merge() that takes
 * another Bucket as an argument, and a clear() method that empties the bucket.
 * If this were Haskell, we'd put a Monoid constraint on Bucket. For performance
 * reasons, it's mutable with separate add() and merge().
 *
 * A little faster if Size is a power of two.
 */
template <typename Bucket, size_t Size>
class BucketedLog {
 public:
  static_assert(
      std::is_default_constructible_v<Bucket>,
      "Bucket must be default-constructible");
  static_assert(
      std::is_copy_constructible_v<Bucket>,
      "Bucket must be copy-constructible");

  /**
   * Advances the internal clock to `now`, clearing buckets that have rolled out
   * of the `Size` window. Then calls `.add(args...)` on the most recent bucket.
   *
   * If the internal clock has already advanced beyond `now`, the call is
   * ignored.
   */
  template <typename... Args>
  void add(uint64_t now, Args&&... args) {
    if (now < windowStart_) {
      // Ignore values from before this window.
      return;
    }
    advanceWindow(now);
    buckets_[now % Size].add(std::forward<Args>(args)...);
  }

  /**
   * Advances the internal clock to `now`, clearing buckets that have rolled
   * out of the `Size` window, and then returns them all. The last bucket in the
   * returned array is the most recent one.
   */
  std::array<Bucket, Size> getAll(uint64_t now) {
    advanceWindow(now);

    std::array<Bucket, Size> result;
    uint64_t b = now + 1;
    for (size_t i = 0; i < Size; ++i) {
      result[i] = buckets_[b % Size];
      ++b;
    }
    return result;
  }

  /**
   * For every bucket in other whose time lines up with a bucket in `this`, call
   * this_bucket.merge(other_bucket).
   */
  void merge(const BucketedLog& other) {
    // Merging brings us at least up to the other log's window.
    advanceWindow(other.windowStart_ + Size - 1);
    for (uint64_t i = windowStart_; i < windowStart_ + Size; ++i) {
      if (i >= other.windowStart_ && i < other.windowStart_ + Size) {
        buckets_[i % Size].merge(other.buckets_[i % Size]);
      }
    }
  }

  /**
   * Clears all buckets in the log.
   */
  void clear() {
    for (auto& bucket : buckets_) {
      bucket.clear();
    }
  }

 private:
  void advanceWindow(uint64_t now) {
    if (now < windowStart_ + Size) {
      return;
    }
    auto newWindowStart = now + 1 - Size;

    DCHECK_GE(newWindowStart, windowStart_);
    uint64_t toClear =
        std::min(static_cast<uint64_t>(Size), newWindowStart - windowStart_);
    DCHECK_GE(newWindowStart, toClear);
    for (uint64_t p = newWindowStart - toClear; p < newWindowStart; ++p) {
      buckets_[p % Size].clear();
    }

    windowStart_ = newWindowStart;
  }

  std::array<Bucket, Size> buckets_;

  /**
   * [windowStart_, windowStart_+Size) is the extent of the sliding window.
   * When `now` >= windowStart_ + Size, the window is advanced and old buckets
   * are cleared.
   */
  uint64_t windowStart_ = 0;
};

} // namespace eden
} // namespace facebook
