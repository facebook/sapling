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

struct FetchStatistics {
  struct Access {
    uint64_t accessCount = 0;
    /**
     * In range [0, 100]. unsigned char is big enough, but prints as a
     * character.
     */
    unsigned short cacheHitRate = 0;
  };

  Access tree;
  Access blob;
  Access metadata;
};

class StatsFetchContext : public ObjectFetchContext {
 public:
  StatsFetchContext() = default;
  StatsFetchContext(const StatsFetchContext& other);

  void didFetch(ObjectType type, const Hash& id, Origin origin) override;

  uint64_t countFetchesOfType(ObjectType type) const;
  uint64_t countFetchesOfTypeAndOrigin(ObjectType type, Origin origin) const;

  FetchStatistics computeStatistics() const;

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
