/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <folly/container/EvictingCacheMap.h>
#include <cstddef>
#include <utility>
#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {

/**
 * A scalable LRU cache for ObjectId indexed data.
 *
 * This is intended to be used for LRU caches that are heavily used across many
 * threads where the lock contention on a single LRU can be seen in benchmarks.
 * Internally, the LRU cache is split into `numShards` to reduce the contention
 * on a single shard. The drawback of this approach is that more sharding leads
 * to an LRU that is less precise since eviction is done at a shard level, not
 * at a global level.
 */
template <class T>
class ShardedLruCache {
 public:
  ShardedLruCache(size_t numShards, size_t maxSize) {
    shards_.reserve(numShards);
    for (size_t i = 0; i < numShards; ++i) {
      shards_.emplace_back(maxSize / numShards);
    }
  }

  void store(const ObjectId& key, T object) {
    auto& shard = getShard(key);
    auto lock = shard.wlock();
    lock->set(key, std::move(object));
  }

  std::optional<T> get(const ObjectId& key) {
    auto& shard = getShard(key);
    auto cache = shard.wlock();
    auto it = cache->find(key);
    if (it == cache->end()) {
      return std::nullopt;
    }
    return it->second;
  }

 private:
  using Cache = folly::EvictingCacheMap<ObjectId, T>;

  struct Shard {
    /*
     * TODO: It never makes sense to rlock an LRU cache, since cache hits mutate
     * the data structure. Thus, should we use a more appropriate type of lock?
     */
    folly::Synchronized<Cache> cache;

    explicit Shard(size_t size) : cache{std::in_place, size} {}
  };

  folly::Synchronized<Cache>& getShard(const ObjectId& key) {
    return shards_[key.getHashCode() % shards_.size()].cache;
  }

  std::vector<Shard> shards_;
};

} // namespace facebook::eden
