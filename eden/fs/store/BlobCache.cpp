/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/BlobCache.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"

namespace facebook::eden {

ObjectCache<Blob, ObjectCacheFlavor::InterestHandle>::GetResult BlobCache::get(
    const ObjectId& hash,
    Interest interest) {
  if (!enabled_) {
    interest = Interest::None;
  }
  auto handle = getInterestHandle(hash, interest);
  if (handle.object) {
    stats_->increment(&ObjectStoreStats::getBlobFromMemory);
  }
  return handle;
}

BlobInterestHandle
BlobCache::insert(ObjectId id, ObjectPtr blob, Interest interest) {
  if (!enabled_) {
    interest = Interest::None;
  }
  return insertInterestHandle(std::move(id), std::move(blob), interest);
}

BlobCache::BlobCache(
    PrivateTag,
    std::shared_ptr<ReloadableConfig> config,
    EdenStatsPtr stats)
    : BlobCache{
          PrivateTag{},
          config->getEdenConfig()->inMemoryBlobCacheSize.getValue(),
          config->getEdenConfig()->inMemoryBlobCacheMinimumItems.getValue(),
          std::move(config),
          std::move(stats)} {}

BlobCache::BlobCache(
    PrivateTag,
    size_t maximumSize,
    size_t minimumCount,
    std::shared_ptr<ReloadableConfig> config,
    EdenStatsPtr stats)
    : ObjectCache<
          Blob,
          ObjectCacheFlavor::InterestHandle>{maximumSize, minimumCount},
      enabled_{config->getEdenConfig()->enableInMemoryBlobCaching.getValue()},
      stats_{std::move(stats)} {
  if (!enabled_) {
    XLOG(DBG2) << "In-memory blob caching is disabled due to configuration";
  }
}

} // namespace facebook::eden
