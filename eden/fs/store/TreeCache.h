/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectCache.h"

namespace facebook::eden {

class ReloadableConfig;

/**
 * An in-memory LRU cache for loaded trees. Currently, this will not be used by
 * the inode code as inodes store the tree data in the inode itself. This is
 * instead used from the thrift side to speed up glob evvaluation.
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
class TreeCache
    : public ObjectCache<Tree, ObjectCacheFlavor::Simple, TreeCacheStats> {
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

 private:
  /**
   * Reference to the eden config, may be a null pointer in unit tests.
   */
  std::shared_ptr<ReloadableConfig> config_;

  explicit TreeCache(
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr stats);

  void registerStats();
};

} // namespace facebook::eden
