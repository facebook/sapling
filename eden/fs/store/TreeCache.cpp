/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/TreeCache.h"
#include <fb303/ServiceData.h>
#include <folly/logging/xlog.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {

static constexpr folly::StringPiece kTreeCacheMemory{"tree_cache.memory"};
static constexpr folly::StringPiece kTreeCacheItems{"tree_cache.items"};

std::shared_ptr<const Tree> TreeCache::get(const ObjectId& id) {
  if (config_->getEdenConfig()->enableInMemoryTreeCaching.getValue()) {
    if (auto shardedCache = std::get_if<ShardedCacheType>(&cache_)) {
      auto result = shardedCache->get(id);
      if (result.has_value()) {
        stats_->increment(&TreeCacheStats::getHit);
        return *result;
      } else {
        stats_->increment(&TreeCacheStats::getMiss);
      }
    } else {
      auto& objectCache = std::get<ObjectCacheType>(cache_);
      return objectCache->getSimple(id);
    }
  }
  return nullptr;
}

void TreeCache::insert(ObjectId id, std::shared_ptr<const Tree> tree) {
  if (config_->getEdenConfig()->enableInMemoryTreeCaching.getValue()) {
    if (auto shardedCache = std::get_if<ShardedCacheType>(&cache_)) {
      auto size = tree->getSizeBytes();
      shardedCache->store(id, std::move(tree));

      auto prevObjectCount =
          objectCount_.fetch_add(1, std::memory_order_relaxed);
      auto prevTotalSize =
          totalSizeInBytes_.fetch_add(size, std::memory_order_relaxed);

      // Check (once) if we exceeded our cache size and set the sharded cache's
      // max size to the previous object count. This basically sets the
      // ShardedLruCache's key based limit using the average size of Trees we
      // have seen so far..
      if (maxSizeBytes_ > 0 && prevTotalSize + size > maxSizeBytes_ &&
          !maxSizeFrozen_.exchange(true, std::memory_order_relaxed)) {
        shardedCache->setMaxSize(prevObjectCount);
      }
    } else {
      auto& objectCache = std::get<ObjectCacheType>(cache_);
      objectCache->insertSimple(std::move(id), std::move(tree));
    }
  }
}

bool TreeCache::contains(const ObjectId& id) const {
  if (config_->getEdenConfig()->enableInMemoryTreeCaching.getValue()) {
    if (auto shardedCache = std::get_if<ShardedCacheType>(&cache_)) {
      return shardedCache->contains(id);
    } else {
      auto& objectCache = std::get<ObjectCacheType>(cache_);
      return objectCache->contains(id);
    }
  }
  return false;
}

void TreeCache::clear() {
  if (config_->getEdenConfig()->enableInMemoryTreeCaching.getValue()) {
    if (auto shardedCache = std::get_if<ShardedCacheType>(&cache_)) {
      shardedCache->clear();
      objectCount_.store(0, std::memory_order_relaxed);
      totalSizeInBytes_.store(0, std::memory_order_relaxed);
    } else {
      auto& objectCache = std::get<ObjectCacheType>(cache_);
      objectCache->clear();
    }
  }
}

size_t TreeCache::maxTreesPerShard() const {
  if (auto shardedCache = std::get_if<ShardedCacheType>(&cache_)) {
    return shardedCache->maxKeysPerShard();
  }
  return 0;
}

TreeCache::TreeCache(
    std::shared_ptr<ReloadableConfig> config,
    EdenStatsPtr stats)
    : config_{config}, stats_{std::move(stats)} {
  auto edenConfig = config->getEdenConfig();
  auto treeCacheShards = edenConfig->treeCacheShards.getValue();
  auto prefetchOptimizations = edenConfig->prefetchOptimizations.getValue();

  // Use ShardedLruCache if prefetch optimizations are enabled and
  // treeCacheShards is non-zero. Otherwise, use the legacy ObjectCache.
  if (prefetchOptimizations && treeCacheShards > 0) {
    maxSizeBytes_ = edenConfig->inMemoryTreeCacheSize.getValue();
    auto pruneCallback =
        [this](const ObjectId&, std::shared_ptr<const Tree>&& tree) {
          auto size = tree->getSizeBytes();
          objectCount_.fetch_sub(1, std::memory_order_relaxed);
          totalSizeInBytes_.fetch_sub(size, std::memory_order_relaxed);
        };
    // Initialize with max size 0 to start with eviction disabled. The
    // ShardedLruCache only supports key-count based eviction, not size. Once
    // TreeCache notices we have crossed our byte size limit, we set the
    // ShardedLruCache's max key count basde on how many trees we have seen.
    cache_ = ShardedCacheType{treeCacheShards, 0, pruneCallback};
  } else {
    cache_ =
        ObjectCache<Tree, ObjectCacheFlavor::Simple, TreeCacheStats>::create(
            edenConfig->inMemoryTreeCacheSize.getValue(),
            edenConfig->inMemoryTreeCacheMinimumItems.getValue(),
            stats_.copy());
  }

  registerStats();
}

TreeCache::~TreeCache() {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->unregisterCallback(kTreeCacheMemory);
  counters->unregisterCallback(kTreeCacheItems);
}

TreeCache::Stats TreeCache::getStats(
    const std::map<std::string, int64_t>& counters) const {
  Stats stats{};

  if (std::holds_alternative<ShardedCacheType>(cache_)) {
    stats.objectCount = objectCount_.load(std::memory_order_relaxed);
    stats.totalSizeInBytes = totalSizeInBytes_.load(std::memory_order_relaxed);

    auto getCounterValue = [&counters](std::string_view name) -> uint64_t {
      auto it = counters.find(std::string(name));
      if (it != counters.end()) {
        return it->second;
      } else {
        return 0;
      }
    };

    stats.hitCount = getCounterValue(stats_->getName(&TreeCacheStats::getHit));
    stats.missCount =
        getCounterValue(stats_->getName(&TreeCacheStats::getMiss));
  } else {
    auto& objectCache = std::get<ObjectCacheType>(cache_);
    auto objectCacheStats = objectCache->getStats(counters);
    stats.objectCount = objectCacheStats.objectCount;
    stats.totalSizeInBytes = objectCacheStats.totalSizeInBytes;
    stats.hitCount = objectCacheStats.hitCount;
    stats.missCount = objectCacheStats.missCount;
    stats.evictionCount = objectCacheStats.evictionCount;
    stats.dropCount = objectCacheStats.dropCount;
  }

  return stats;
}

void TreeCache::registerStats() {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();

  if (std::holds_alternative<ShardedCacheType>(cache_)) {
    counters->registerCallback(kTreeCacheMemory, [this] {
      return totalSizeInBytes_.load(std::memory_order_relaxed);
    });
    counters->registerCallback(kTreeCacheItems, [this] {
      return objectCount_.load(std::memory_order_relaxed);
    });
  } else {
    auto& objectCache = std::get<ObjectCacheType>(cache_);
    counters->registerCallback(kTreeCacheMemory, [objectCache] {
      return objectCache->getTotalSizeBytes();
    });
    counters->registerCallback(kTreeCacheItems, [objectCache] {
      return objectCache->getObjectCount();
    });
  }
}

} // namespace facebook::eden
