/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <atomic>
#include "eden/fs/store/IObjectStore.h"

namespace facebook {
namespace eden {

class StatsFetchContext : public ObjectFetchContext {
 public:
  StatsFetchContext() = default;
  StatsFetchContext(const StatsFetchContext& other);

  void didFetch(ObjectType type, const Hash& id, Origin origin) override;

  uint64_t countFetchesOfType(ObjectType type) const;

  /**
   * Sums the counts from another fetch context into this one.
   */
  void merge(const StatsFetchContext& other);

 private:
  std::atomic<uint64_t> counts_[ObjectFetchContext::kObjectTypeEnumMax]
                               [ObjectFetchContext::kOriginEnumMax] = {};
};

} // namespace eden
} // namespace facebook
