/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/TreeCache.h"

#include <chrono>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {

static constexpr folly::StringPiece kTreeCacheMemory{"tree_cache.memory"};
static constexpr folly::StringPiece kTreeCacheItems{"tree_cache.items"};

namespace {

bool treeHasRestrictedChild(const Tree& tree) {
  for (const auto& entry : tree) {
    if (entry.second.isRestricted()) {
      return true;
    }
  }
  return false;
}

} // namespace

std::shared_ptr<const Tree> TreeCache::get(const ObjectId& id) {
  if (config_->getEdenConfig()->enableInMemoryTreeCaching.getValue()) {
    return getSimple(id);
  }
  return nullptr;
}

void TreeCache::insert(ObjectId id, std::shared_ptr<const Tree> tree) {
  auto config = config_->getEdenConfig();
  if (!config->enableInMemoryTreeCaching.getValue()) {
    return;
  }
  if (tree->isRestricted()) {
    // The empty placeholder for a denied tree is fetched by id when access is
    // later granted (TreeInode::transitionToUnrestricted); served from here it
    // would become the directory's contents.
    return;
  }
  if (treeHasRestrictedChild(*tree)) {
    // A restricted entry records an ACL denial that the rest of EdenFS
    // re-evaluates every restrictedTreeTtlSeconds; a cached copy must not
    // outlive that interval.
    auto ttl =
        std::chrono::seconds{config->restrictedTreeTtlSeconds.getValue()};
    if (ttl.count() == 0) {
      return;
    }
    return insertSimple(std::move(id), std::move(tree), Clock::now() + ttl);
  }
  insertSimple(std::move(id), std::move(tree));
}

TreeCache::TreeCache(std::shared_ptr<ReloadableConfig> config, EdenStatsPtr stats)
      : ObjectCache<Tree, ObjectCacheFlavor::Simple, TreeCacheStats>{
            config->getEdenConfig()->inMemoryTreeCacheSize.getValue(),
            config->getEdenConfig()->inMemoryTreeCacheMinimumItems.getValue(),
            std::move(stats),
            [&config]() -> size_t {
              auto shards =
                  config->getEdenConfig()->treeCacheShards.getValue();
              return shards > 0 ? shards : 1;
            }()},
        config_{config} {
  registerStats();
}

TreeCache::~TreeCache() {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->unregisterCallback(kTreeCacheMemory);
  counters->unregisterCallback(kTreeCacheItems);
}

void TreeCache::registerStats() {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->registerCallback(
      kTreeCacheMemory, [this] { return getTotalSizeBytes(); });
  counters->registerCallback(
      kTreeCacheItems, [this] { return getObjectCount(); });
}

} // namespace facebook::eden
