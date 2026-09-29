/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EventRateLimiter.h"

#include <algorithm>

namespace facebook::eden {

EventRateLimiter::EventRateLimiter(Clock clock) : clock_{std::move(clock)} {}

std::optional<uint64_t> EventRateLimiter::tryAcquire(
    std::string_view key,
    double ratePerSecond,
    double burst) {
  if (ratePerSecond <= 0) {
    auto* bucket = findBucket(key);
    return bucket ? bucket->suppressed.exchange(0, std::memory_order_relaxed)
                  : 0;
  }
  burst = std::max(burst, 1.0);
  double now = clock_();
  // A bucket holds (now - zeroTime) * rate tokens, so a new one is started
  // far enough in the past to be full at the first event.
  auto& bucket = getBucket(key, now - burst / ratePerSecond);
  if (!bucket.tokens.consume(1.0, ratePerSecond, burst, now)) {
    bucket.suppressed.fetch_add(1, std::memory_order_relaxed);
    return std::nullopt;
  }
  return bucket.suppressed.exchange(0, std::memory_order_relaxed);
}

EventRateLimiter::Bucket* EventRateLimiter::findBucket(std::string_view key) {
  auto buckets = buckets_.rlock();
  auto it = buckets->find(key);
  return it == buckets->end() ? nullptr : it->second.get();
}

EventRateLimiter::Bucket& EventRateLimiter::getBucket(
    std::string_view key,
    double zeroTime) {
  if (auto* bucket = findBucket(key)) {
    return *bucket;
  }
  auto buckets = buckets_.wlock();
  auto& slot = (*buckets)[std::string{key}];
  if (!slot) {
    slot = std::make_unique<Bucket>(zeroTime);
  }
  return *slot;
}

} // namespace facebook::eden
