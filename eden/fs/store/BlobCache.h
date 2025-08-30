/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/model/Blob.h"
#include "eden/fs/store/ObjectCache.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {

class ReloadableConfig;

using BlobInterestHandle = ObjectInterestHandle<Blob, BlobCacheStats>;

/**
 * An in-memory LRU cache for loaded blobs. It is parameterized by both a
 * maximum cache size and a minimum entry count. The cache tries to evict
 * entries when the total number of loaded blobs exceeds the maximum cache size,
 * except that it always keeps the minimum entry count around.
 *
 * The intent of the minimum entry count is to avoid having to reload
 * frequently-accessed large blobs when they are larger than the maximum cache
 * size.
 *
 * It is safe to use this object from arbitrary threads.
 */
class BlobCache : public ObjectCache<
                      Blob,
                      ObjectCacheFlavor::InterestHandle,
                      BlobCacheStats> {
  struct PrivateTag {};

 public:
  static std::shared_ptr<BlobCache> create(
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr stats) {
    return std::make_shared<BlobCache>(
        PrivateTag{}, std::move(config), std::move(stats));
  }
  static std::shared_ptr<BlobCache> create(
      size_t maximumSize,
      size_t minimumCount,
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr stats) {
    return std::make_shared<BlobCache>(
        PrivateTag{},
        maximumSize,
        minimumCount,
        std::move(config),
        std::move(stats));
  }

  explicit BlobCache(
      PrivateTag,
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr stats);
  explicit BlobCache(
      PrivateTag,
      size_t maximumSize,
      size_t minimumCount,
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr stats);
  ~BlobCache();

  /**
   * If a blob for the given id is in cache, return it. If the blob is not in
   * cache, return nullptr (and an empty interest handle).
   *
   * If a blob is returned and interest is WantHandle, then a movable handle
   * object is also returned. When the interest handle is destroyed, the cached
   * blob may be evicted.
   *
   * After fetching a blob, prefer calling getBlob() on the returned
   * BlobInterestHandle first. It can avoid some overhead or return a blob if
   * it still exists in memory and the BlobCache has evicted its reference.
   */
  GetResult get(
      const ObjectId& id,
      Interest interest = Interest::LikelyNeededAgain);

  /**
   * Inserts a blob into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted.
   *
   * Optionally returns an interest handle that, when dropped, evicts the
   * inserted blob.
   */
  BlobInterestHandle insert(
      ObjectId id,
      ObjectPtr blob,
      Interest interest = Interest::LikelyNeededAgain);

 private:
  /**
   * Populated via EdenConfig at object creation time. This could be changed to
   * be reloadable if the minimum and maximum cache sizes are also changed to be
   * reloadable.
   */
  bool enabled_;

  EdenStatsPtr stats_;

  void registerStats();
};

} // namespace facebook::eden
