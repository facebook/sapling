/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectCache.h"

namespace facebook::eden {

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
 * be cachable your minimum entry count must be atleast 1, otherwise insert may
 * not actually insert the tree into the cache.
 *
 * It is safe to use this object from arbitrary threads.
 */
class TreeCache : public ObjectCache<Tree, ObjectCacheFlavor::Simple> {
 public:
  static std::shared_ptr<TreeCache> create(
      std::shared_ptr<ReloadableConfig> config) {
    struct TC : TreeCache {
      explicit TC(std::shared_ptr<ReloadableConfig> c) : TreeCache{c} {}
    };
    return std::make_shared<TC>(config);
  }
  ~TreeCache() = default;

  /**
   * If a tree for the given hash is in cache, return it. If the tree is not in
   * cache, return nullptr.
   */
  std::shared_ptr<const Tree> get(const ObjectId& hash);

  /**
   * Inserts a tree into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted.
   */
  void insert(std::shared_ptr<const Tree> tree);

 private:
  /**
   * Reference to the eden config, may be a null pointer in unit tests.
   */
  std::shared_ptr<ReloadableConfig> config_;

  explicit TreeCache(std::shared_ptr<ReloadableConfig> config);
};

} // namespace facebook::eden
