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

BlobCache::BlobCache(
    PrivateTag,
    std::shared_ptr<ReloadableConfig> config,
    EdenStatsPtr stats)
    : BlobCache{
          PrivateTag{},
          config->getEdenConfig()->inMemoryBlobCacheSize.getValue(),
          config->getEdenConfig()->inMemoryBlobCacheMinimumItems.getValue(),
          std::move(stats)} {}

BlobCache::BlobCache(
    PrivateTag,
    size_t maximumSize,
    size_t minimumCount,
    EdenStatsPtr stats)
    : ObjectCache<
          Blob,
          ObjectCacheFlavor::InterestHandle>{maximumSize, minimumCount},
      stats_{std::move(stats)} {}

} // namespace facebook::eden
