/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <cstddef>
#include <list>
#include <unordered_map>
#include "eden/fs/model/Hash.h"

namespace facebook {
namespace eden {

class Blob;
class BlobCache;

/**
 * Cache lookups return a BlobInterestHandle which should be held as long as the
 * blob remains interesting.
 */
class BlobInterestHandle {
 public:
  BlobInterestHandle() noexcept = default;

  ~BlobInterestHandle() noexcept {
    reset();
  }

  BlobInterestHandle(BlobInterestHandle&& other) noexcept = default;
  BlobInterestHandle& operator=(BlobInterestHandle&& other) noexcept = default;

  /**
   * If this is a valid interest handle, and the blob is still in cache, return
   * the corresponding blob and move it to the back of the eviction queue.
   *
   * Otherwise, return nullptr.
   */
  std::shared_ptr<const Blob> getBlob() const;

  void reset() noexcept;

 private:
  BlobInterestHandle(
      std::weak_ptr<BlobCache> blobCache,
      const Hash& hash,
      std::weak_ptr<const Blob> blob,
      uint64_t generation) noexcept;

  std::weak_ptr<BlobCache> blobCache_;

  // hash_ is only accessed if blobCache_ is non-expired.
  Hash hash_;

  // In the situation that the Blob exists even if it's been evicted, allow
  // retrieving it anyway.
  std::weak_ptr<const Blob> blob_;

  // Only causes eviction if this matches the corresponding
  // CacheItem::generation.
  uint64_t cacheItemGeneration_{0};

  friend class BlobCache;
};

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
class BlobCache : public std::enable_shared_from_this<BlobCache> {
 public:
  using BlobPtr = std::shared_ptr<const Blob>;

  enum class Interest {
    /**
     * Will return a blob if it is cached, but not add a reference to it nor
     * move it to the back of the eviction queue.
     */
    UnlikelyNeededAgain,

    /**
     * If a blob is cached, its reference count is incremented and a handle is
     * returned that, when dropped, releases the reference and evicts the item
     * from cache. Intended for satisfying a series of blob reads from cache
     * until the inode is unloaded, after which the blob can evicted from cache,
     * freeing space.
     */
    WantHandle,

    /**
     * If a blob is cached, its reference count is incremented, but no interest
     * handle is returned. It is assumed to be worth caching until it is
     * naturally evicted.
     */
    LikelyNeededAgain,
  };

  struct GetResult {
    BlobPtr blob;
    BlobInterestHandle interestHandle;

    GetResult(GetResult&&) = default;
    GetResult& operator=(GetResult&&) = default;
  };

  struct Stats {
    size_t blobCount{0};
    size_t totalSizeInBytes{0};
    uint64_t hitCount{0};
    uint64_t missCount{0};
    uint64_t evictionCount{0};
    uint64_t dropCount{0};
  };

  static std::shared_ptr<BlobCache> create(
      size_t maximumCacheSizeBytes,
      size_t minimumEntryCount);
  ~BlobCache();

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
      const Hash& hash,
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
      BlobPtr blob,
      Interest interest = Interest::LikelyNeededAgain);

  /**
   * Returns true if the cache contains a blob for the given hash.
   */
  bool contains(const Hash& hash) const;

  /**
   * Evicts everything from cache.
   */
  void clear();

  /**
   * Return information about the current size of the cache and the total number
   * of hits and misses.
   */
  Stats getStats() const;

 private:
  /*
   * TODO: This data structure could be implemented more efficiently. But since
   * most of the data will be held in the blobs themselves and not in this
   * index, the overhead is not worrisome.
   *
   * But should we ever decide to optimize it, storing the array of CacheItem
   * nodes in a std::vector with indices to its siblings and to the next node
   * in the hash chain would be more efficient, especially since the indices
   * could be smaller than a pointer.
   */

  struct CacheItem {
    // WARNING: leaves index unset. Since the items map and evictionQueue are
    // circular, initialization of index must happen after the CacheItem is
    // constructed.
    explicit CacheItem(BlobPtr b, uint64_t g)
        : blob{std::move(b)}, generation{g} {}

    BlobPtr blob;
    std::list<CacheItem*>::iterator index;

    /// Incremented on every LikelyNeededAgain or WantInterestHandle.
    /// Decremented on every dropInterestHandle. Evicted if it reaches zero.
    uint64_t referenceCount{0};

    /// Given a unique value upon allocation. Used to verify InterestHandle
    // matches this specific item.
    uint64_t generation{0};
  };

  struct State {
    size_t totalSize{0};
    std::unordered_map<Hash, CacheItem> items;

    /// Entries are evicted from the front of the queue.
    std::list<CacheItem*> evictionQueue;

    uint64_t hitCount{0};
    uint64_t missCount{0};
    uint64_t evictionCount{0};
    uint64_t dropCount{0};
  };

  void dropInterestHandle(const Hash& hash, uint64_t generation) noexcept;

  explicit BlobCache(size_t maximumCacheSizeBytes, size_t minimumEntryCount);
  void evictUntilFits(State& state) noexcept;
  void evictOne(State& state) noexcept;
  void evictItem(State&, CacheItem* item) noexcept;

  const size_t maximumCacheSizeBytes_;
  const size_t minimumEntryCount_;
  folly::Synchronized<State> state_;

  friend class BlobInterestHandle;
};

} // namespace eden
} // namespace facebook
