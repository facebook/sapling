/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "eden/fs/model/Blob.h"
#include "eden/fs/store/ObjectCache.h"

namespace facebook::eden {

using BlobInterestHandle = ObjectInterestHandle<Blob>;

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
class BlobCache : public ObjectCache<Blob, ObjectCacheFlavor::InterestHandle> {
 public:
  static std::shared_ptr<BlobCache> create(
      size_t maximumCacheSizeBytes,
      size_t minimumEntryCount) {
    struct BC : BlobCache {
      BC(size_t x, size_t y) : BlobCache{x, y} {}
    };
    return std::make_shared<BC>(maximumCacheSizeBytes, minimumEntryCount);
  }
  ~BlobCache() = default;

  /**
   * If a blob for the given hash is in cache, return it. If the blob is not in
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
      const ObjectId& hash,
      Interest interest = Interest::LikelyNeededAgain) {
    return getInterestHandle(hash, interest);
  }

  /**
   * Inserts a blob into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted.
   *
   * Optionally returns an interest handle that, when dropped, evicts the
   * inserted blob.
   */
  BlobInterestHandle insert(
      ObjectPtr blob,
      Interest interest = Interest::LikelyNeededAgain) {
    return insertInterestHandle(blob, interest);
  }

 private:
  explicit BlobCache(size_t maximumCacheSizeBytes, size_t minimumEntryCount)
      : ObjectCache<Blob, ObjectCacheFlavor::InterestHandle>{
            maximumCacheSizeBytes,
            minimumEntryCount} {}
};

} // namespace facebook::eden
