/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/IDGen.h"
#include <folly/Likely.h>
#include <folly/lang/Align.h>
#include <atomic>

namespace {
/**
 * Allocating one unique ID per nanosecond would wrap around in over 500 years.
 *
 * Allocated to its own cache line.
 */
struct alignas(folly::hardware_destructive_interference_size) {
  std::atomic<uint64_t> counter{0};
} global;

thread_local uint64_t localCounter{0};

/**
 * Number of unique IDs to hand out to a thread at a time. This avoids cache
 * line contention on globalCounter. kRangeSize should be large enough to reduce
 * contention but small enough that the pathological case of threads being
 * spawned in a tight loop, each allocating one unique ID, does not rapidly
 * exhaust the 64-bit counter space.
 *
 * I haven't measured, but I'd be surprised if a thread could be created in
 * 2000 nanoseconds.
 */
constexpr uint64_t kRangeSize = 2048;

static_assert(
    (kRangeSize & (kRangeSize - 1)) == 0,
    "kRangeSize must be a power of two");
} // namespace

namespace facebook {
namespace eden {

uint64_t generateUniqueID() noexcept {
  uint64_t current = localCounter;
  if (UNLIKELY(current % kRangeSize == 0)) {
    current = global.counter.fetch_add(kRangeSize, std::memory_order_relaxed);
  }
  ++current;
  localCounter = current;
  return current;
}

} // namespace eden
} // namespace facebook
