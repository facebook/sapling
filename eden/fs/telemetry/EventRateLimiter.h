/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <atomic>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>

#include <folly/Synchronized.h>
#include <folly/TokenBucket.h>
#include <folly/container/F14Map.h>

namespace facebook::eden {

/**
 * Per-key token bucket rate limiter for telemetry events.
 *
 * Each key (typically an event type) gets its own bucket, so a storm of one
 * kind of event cannot crowd out the others. Denied events are counted, and
 * the count is handed back with the next admitted event for that key so the
 * caller can record how many were dropped.
 *
 * Thread-safe. Buckets are created on first use and never removed, so the key
 * space must stay small (one entry per event type).
 */
class EventRateLimiter {
 public:
  using Clock = std::function<double()>;

  explicit EventRateLimiter(
      Clock clock = &folly::DynamicTokenBucket::defaultClockNow);

  /**
   * Decide whether an event for `key` may be emitted.
   *
   * If the event is admitted, returns the number of events for `key` that
   * were suppressed since the previous admitted one. Returns std::nullopt if
   * the event should be dropped.
   *
   * A `ratePerSecond` of zero or less admits everything, still reporting any
   * count accumulated while limiting was active. `burst` is the number of
   * events admitted immediately from an idle bucket and is clamped to at
   * least 1.
   */
  std::optional<uint64_t>
  tryAcquire(std::string_view key, double ratePerSecond, double burst);

 private:
  struct Bucket {
    explicit Bucket(double zeroTime) : tokens{zeroTime} {}

    folly::DynamicTokenBucket tokens;
    std::atomic<uint64_t> suppressed{0};
  };

  Bucket* findBucket(std::string_view key);
  Bucket& getBucket(std::string_view key, double zeroTime);

  Clock clock_;
  folly::Synchronized<folly::F14FastMap<std::string, std::unique_ptr<Bucket>>>
      buckets_;
};

} // namespace facebook::eden
