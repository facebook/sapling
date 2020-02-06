/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/StatsFetchContext.h"
#include <folly/logging/xlog.h>

namespace facebook {
namespace eden {

StatsFetchContext::StatsFetchContext(const StatsFetchContext& other) {
  for (size_t y = 0; y < ObjectFetchContext::kObjectTypeEnumMax; ++y) {
    for (size_t x = 0; x < ObjectFetchContext::kOriginEnumMax; ++x) {
      // This could almost certainly use a more relaxed memory ordering.
      counts_[y][x] = other.counts_[y][x].load();
    }
  }
}

void StatsFetchContext::didFetch(ObjectType type, const Hash&, Origin origin) {
  XCHECK(type < ObjectFetchContext::kObjectTypeEnumMax)
      << "type is out of range: " << type;
  XCHECK(origin < ObjectFetchContext::kOriginEnumMax)
      << "origin is out of range: " << type;
  counts_[type][origin].fetch_add(1, std::memory_order_acq_rel);
}

uint64_t StatsFetchContext::countFetchesOfType(ObjectType type) const {
  XCHECK(type < ObjectFetchContext::kObjectTypeEnumMax)
      << "type is out of range: " << type;
  uint64_t result = 0;
  for (unsigned origin = 0; origin < ObjectFetchContext::kOriginEnumMax;
       ++origin) {
    result += counts_[type][origin].load(std::memory_order_acquire);
  }
  return result;
}

void StatsFetchContext::merge(const StatsFetchContext& other) {
  for (unsigned type = 0; type < ObjectFetchContext::kObjectTypeEnumMax;
       ++type) {
    for (unsigned origin = 0; origin < ObjectFetchContext::kOriginEnumMax;
         ++origin) {
      counts_[type][origin] += other.counts_[type][origin];
    }
  }
}

} // namespace eden
} // namespace facebook
