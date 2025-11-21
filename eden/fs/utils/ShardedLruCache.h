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
  using PruneHookCall = std::function<void(const ObjectId&, T&&)>;

  ShardedLruCache(
      size_t numShards,
      size_t maxSize,
      PruneHookCall pruneHook = nullptr) {
    shards_.reserve(numShards);
    for (size_t i = 0; i < numShards; ++i) {
      shards_.emplace_back(maxSize / numShards, pruneHook);
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

  bool contains(const ObjectId& key) const {
    auto& shard = getShard(key);
    auto cache = shard.rlock();
    return cache->exists(key);
  }

  void clear() {
    for (auto& shard : shards_) {
      auto cache = shard.cache.wlock();
      cache->clear();
    }
  }

  /**
   * Get the max size of the first shard. Used for testing to verify
   * that max size is being set correctly.
   */
  size_t maxKeysPerShard() const {
    if (shards_.empty()) {
      return 0;
    }
    auto cache = shards_[0].cache.rlock();
    return cache->getMaxSize();
  }

  /**
   * Update the maximum size of the cache. The maxSize is divided
   * evenly amongst the shards. If maxSize is 0, disable eviction.
   */
  void setMaxSize(size_t maxSize) {
    // Ensure each shard gets at least one item.
    size_t perShardSize =
        maxSize == 0 ? 0 : std::max(maxSize / shards_.size(), size_t(1));
    for (auto& shard : shards_) {
      auto cache = shard.cache.wlock();
      cache->setMaxSize(perShardSize);
    }
  }

 private:
  using Cache = folly::EvictingCacheMap<ObjectId, T>;

  struct Shard {
    folly::Synchronized<Cache> cache;

    explicit Shard(size_t size, PruneHookCall pruneHook)
        : cache{std::in_place, size} {
      if (pruneHook) {
        cache.wlock()->setPruneHook(pruneHook);
      }
    }
  };

  folly::Synchronized<Cache>& getShard(const ObjectId& key) {
    return shards_[key.getHashCode() % shards_.size()].cache;
  }

  const folly::Synchronized<Cache>& getShard(const ObjectId& key) const {
    return shards_[key.getHashCode() % shards_.size()].cache;
  }

  std::vector<Shard> shards_;
};

} // namespace facebook::eden
