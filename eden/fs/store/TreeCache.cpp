/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/TreeCache.h"

#include "eden/fs/config/EdenConfig.h"

namespace facebook::eden {
std::shared_ptr<const Tree> TreeCache::get(const ObjectId& hash) {
  if (config_->getEdenConfig()->enableInMemoryTreeCaching.getValue()) {
    return getSimple(hash);
  }
  return std::shared_ptr<const Tree>{nullptr};
}

void TreeCache::insert(std::shared_ptr<const Tree> tree) {
  if (config_->getEdenConfig()->enableInMemoryTreeCaching.getValue()) {
    return insertSimple(tree);
  }
}

TreeCache::TreeCache(std::shared_ptr<ReloadableConfig> config)
      : ObjectCache<Tree, ObjectCacheFlavor::Simple>{
            config->getEdenConfig()->inMemoryTreeCacheSize.getValue(),
            config->getEdenConfig()->inMemoryTreeCacheMinElements.getValue()},
        config_{config} {}

} // namespace facebook::eden
