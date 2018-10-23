/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/IDGen.h"
#include <folly/CachelinePadded.h>
#include <folly/ThreadLocal.h>
#include <atomic>

namespace {
/**
 * Allocating one unique ID per nanosecond would wrap around in over 500 years.
 *
 * CachelinePadded may be excessive here.
 */
folly::CachelinePadded<std::atomic<uint64_t>> globalCounter;

struct LocalRange {
  uint64_t begin{0};
  uint64_t end{0};
};

folly::ThreadLocal<LocalRange> localRange;

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
constexpr uint64_t kRangeSize = 2000;
} // namespace

namespace facebook {
namespace eden {

uint64_t generateUniqueID() {
  auto range = localRange.get();
  if (UNLIKELY(range->begin == range->end)) {
    auto begin =
        globalCounter->fetch_add(kRangeSize, std::memory_order_relaxed);
    range->begin = begin;
    range->end = begin + kRangeSize;
  }
  return range->begin++;
}

} // namespace eden
} // namespace facebook
