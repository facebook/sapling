/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <map>
#include <memory>
#include <variant>
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectCache.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/ShardedLruCache.h"

namespace facebook::eden {

class ReloadableConfig;

/**
 * An in-memory LRU cache for loaded trees. Currently, this will not be used by
 * the inode code as inodes store the tree data in the inode itself. This is
 * instead used from the thrift side to speed up glob evaluation.
 *
 * It is parameterized by both a maximum cache size and a minimum entry count.
 * The cache tries to evict entries when the total number of loaded trees
 * exceeds the maximum cache size, except that it always keeps the minimum
 * entry count around.
 *
 * The intent of the minimum entry count is to avoid having to reload
 * frequently-accessed large trees when they are larger than the maximum cache
 * size. Note that if you want trees larger than the maximum size in bytes to
 * be cacheable your minimum entry count must be at least 1, otherwise insert
 * may not actually insert the tree into the cache.
 *
 * It is safe to use this object from arbitrary threads.
 */
class TreeCache {
 public:
  static std::shared_ptr<TreeCache> create(
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr stats) {
    struct TC : TreeCache {
      explicit TC(std::shared_ptr<ReloadableConfig> c, EdenStatsPtr s)
          : TreeCache{c, std::move(s)} {}
    };
    return std::make_shared<TC>(config, std::move(stats));
  }
  ~TreeCache();
  TreeCache(const TreeCache&) = delete;
  TreeCache& operator=(const TreeCache&) = delete;
  TreeCache(TreeCache&&) = delete;
  TreeCache& operator=(TreeCache&&) = delete;

  /**
   * If a tree for the given id is in cache, return it. If the tree is not in
   * cache, return nullptr.
   */
  std::shared_ptr<const Tree> get(const ObjectId& id);

  /**
   * Inserts a tree into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted.
   */
  void insert(ObjectId id, std::shared_ptr<const Tree> tree);

  /**
   * Returns true if the cache contains a tree for the given id.
   */
  bool contains(const ObjectId& id) const;

  /**
   * Evicts everything from cache.
   */
  void clear();

  struct Stats {
    size_t objectCount{0};
    size_t totalSizeInBytes{0};
    uint64_t hitCount{0};
    uint64_t missCount{0};
    uint64_t evictionCount{0};
    uint64_t dropCount{0};
  };

  /**
   * Return information about the current size of the cache and the total number
   * of hits and misses.
   */
  Stats getStats(const std::map<std::string, int64_t>& counters) const;

 private:
  /**
   * Reference to the eden config, may be a null pointer in unit tests.
   */
  std::shared_ptr<ReloadableConfig> config_;

  using ObjectCacheType = std::shared_ptr<
      ObjectCache<Tree, ObjectCacheFlavor::Simple, TreeCacheStats>>;
  using ShardedCacheType = ShardedLruCache<std::shared_ptr<const Tree>>;

  /**
   * The underlying cache implementation. Either ShardedLruCache (when
   * prefetch optimizations are enabled and treeCacheShards > 0) or
   * ObjectCache (legacy implementation).
   */
  std::variant<ObjectCacheType, ShardedCacheType> cache_;

  EdenStatsPtr stats_;

  std::atomic<size_t> objectCount_{0};
  std::atomic<size_t> totalSizeInBytes_{0};

  explicit TreeCache(
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr stats);

  void registerStats();
};

} // namespace facebook::eden
