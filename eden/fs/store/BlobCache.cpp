/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "BlobCache.h"
#include <folly/MapUtil.h>
#include <folly/logging/xlog.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/utils/IDGen.h"

namespace facebook {
namespace eden {

BlobInterestHandle::BlobInterestHandle(
    std::weak_ptr<BlobCache> blobCache,
    const Hash& hash,
    std::weak_ptr<const Blob> blob,
    uint64_t generation) noexcept
    : blobCache_{std::move(blobCache)},
      hash_{hash},
      blob_{std::move(blob)},
      cacheItemGeneration_{generation} {}

void BlobInterestHandle::reset() noexcept {
  if (auto blobCache = blobCache_.lock()) {
    blobCache->dropInterestHandle(hash_, cacheItemGeneration_);
  }
  blobCache_.reset();
}

std::shared_ptr<const Blob> BlobInterestHandle::getBlob() const {
  auto blobCache = blobCache_.lock();
  if (blobCache) {
    // UnlikelyNeededAgain because there's no need to create a new interest
    // handle nor bump the refcount.
    auto blob =
        blobCache->get(hash_, BlobCache::Interest::UnlikelyNeededAgain).blob;
    if (blob) {
      return blob;
    }
  }

  // If the blob is no longer in cache, at least see if it's still in memory.
  return blob_.lock();
}

std::shared_ptr<BlobCache> BlobCache::create(
    size_t maximumCacheSizeBytes,
    size_t minimumEntryCount) {
  // Allow make_shared with private constructor.
  struct BC : BlobCache {
    BC(size_t x, size_t y) : BlobCache{x, y} {}
  };
  return std::make_shared<BC>(maximumCacheSizeBytes, minimumEntryCount);
}

BlobCache::BlobCache(size_t maximumCacheSizeBytes, size_t minimumEntryCount)
    : maximumCacheSizeBytes_{maximumCacheSizeBytes},
      minimumEntryCount_{minimumEntryCount} {}

BlobCache::~BlobCache() {}

BlobCache::GetResult BlobCache::get(const Hash& hash, Interest interest) {
  XLOG(DBG6) << "BlobCache::get " << hash;

  // Acquires BlobCache's lock upon destruction by calling dropInterestHandle,
  // so ensure that, if an exception is thrown below, the ~BlobInterestHandle
  // runs after the lock is released.
  BlobInterestHandle interestHandle;

  auto state = state_.wlock();

  auto* item = folly::get_ptr(state->items, hash);
  if (!item) {
    XLOG(DBG6) << "BlobCache::get missed";
    ++state->missCount;
    return GetResult{};
  }

  switch (interest) {
    case Interest::UnlikelyNeededAgain:
      interestHandle.blob_ = item->blob;
      break;
    case Interest::WantHandle:
      interestHandle = BlobInterestHandle{
          shared_from_this(), hash, item->blob, item->generation};
      ++item->referenceCount;
      break;
    case Interest::LikelyNeededAgain:
      interestHandle.blob_ = item->blob;
      // Bump the reference count without allocating an interest handle - this
      // will cause the reference count to never reach zero, avoiding early
      // eviction.
      //
      // TODO: One possible optimization here is to set a bit (reference count
      // to UINT64_MAX) after which new interest handles never need to be
      // created.
      ++item->referenceCount;
      break;
  }

  XLOG(DBG6) << "BlobCache::get hit";

  // TODO: Should we avoid promoting if interest is UnlikelyNeededAgain?
  // For now, we'll try not to be too clever.
  state->evictionQueue.splice(
      state->evictionQueue.end(), state->evictionQueue, item->index);
  ++state->hitCount;
  return GetResult{item->blob, std::move(interestHandle)};
}

BlobInterestHandle BlobCache::insert(
    std::shared_ptr<const Blob> blob,
    Interest interest) {
  XLOG(DBG6) << "BlobCache::insert " << blob->getHash();

  // Acquires BlobCache's lock upon destruction by calling dropInterestHandle,
  // so ensure that, if an exception is thrown below, the ~BlobInterestHandle
  // runs after the lock is released.
  BlobInterestHandle interestHandle;

  auto hash = blob->getHash();
  auto size = blob->getSize();

  auto cacheItemGeneration = generateUniqueID();

  if (interest == Interest::WantHandle) {
    // This can throw, so do it before inserting into items.
    interestHandle =
        BlobInterestHandle{shared_from_this(), hash, blob, cacheItemGeneration};
  } else {
    interestHandle.blob_ = blob;
  }

  XLOG(DBG6) << "  creating entry with generation=" << cacheItemGeneration;

  auto state = state_.wlock();
  auto [iter, inserted] =
      state->items.try_emplace(hash, std::move(blob), cacheItemGeneration);
  // noexcept from here until `try`
  switch (interest) {
    case Interest::UnlikelyNeededAgain:
      break;
    case Interest::WantHandle:
    case Interest::LikelyNeededAgain:
      ++iter->second.referenceCount;
      break;
  }
  auto* itemPtr = &iter->second;
  if (inserted) {
    try {
      state->evictionQueue.push_back(itemPtr);
    } catch (std::exception&) {
      state->items.erase(iter);
      throw;
    }
    iter->second.index = std::prev(state->evictionQueue.end());
    state->totalSize += size;
    evictUntilFits(*state);
  } else {
    XLOG(DBG6) << "  duplicate entry, using generation " << itemPtr->generation;
    // Inserting duplicate entry - use its generation.
    interestHandle.cacheItemGeneration_ = itemPtr->generation;
    state->evictionQueue.splice(
        state->evictionQueue.end(), state->evictionQueue, itemPtr->index);
  }
  return interestHandle;
}

bool BlobCache::contains(const Hash& hash) const {
  auto state = state_.rlock();
  return 1 == state->items.count(hash);
}

void BlobCache::clear() {
  XLOG(DBG6) << "BlobCache::clear";
  auto state = state_.wlock();
  state->totalSize = 0;
  state->items.clear();
  state->evictionQueue.clear();
}

BlobCache::Stats BlobCache::getStats() const {
  auto state = state_.rlock();
  Stats stats;
  stats.blobCount = state->items.size();
  stats.totalSizeInBytes = state->totalSize;
  stats.hitCount = state->hitCount;
  stats.missCount = state->missCount;
  stats.evictionCount = state->evictionCount;
  stats.dropCount = state->dropCount;
  return stats;
}

void BlobCache::dropInterestHandle(
    const Hash& hash,
    uint64_t generation) noexcept {
  XLOG(DBG6) << "dropInterestHandle " << hash << " generation=" << generation;
  auto state = state_.wlock();

  auto* item = folly::get_ptr(state->items, hash);
  if (!item) {
    // Cached item already evicted.
    return;
  }

  if (generation != item->generation) {
    // Item was evicted and re-added between creating and dropping the interest
    // handle.
    return;
  }

  if (item->referenceCount == 0) {
    XLOG(WARN)
        << "Reference count on item for " << hash
        << " was already zero: an exception must have been thrown during get()";
    return;
  }

  if (--item->referenceCount == 0) {
    state->evictionQueue.erase(item->index);
    ++state->dropCount;
    evictItem(*state, item);
  }
}

void BlobCache::evictUntilFits(State& state) noexcept {
  XLOG(DBG6) << "state.totalSize=" << state.totalSize
             << ", maximumCacheSizeBytes_=" << maximumCacheSizeBytes_
             << ", evictionQueue.size()=" << state.evictionQueue.size()
             << ", minimumEntryCount_=" << minimumEntryCount_;
  while (state.totalSize > maximumCacheSizeBytes_ &&
         state.evictionQueue.size() > minimumEntryCount_) {
    evictOne(state);
  }
}

void BlobCache::evictOne(State& state) noexcept {
  CacheItem* front = state.evictionQueue.front();
  state.evictionQueue.pop_front();
  ++state.evictionCount;
  evictItem(state, front);
}

void BlobCache::evictItem(State& state, CacheItem* item) noexcept {
  XLOG(DBG6) << "evicting " << item->blob->getHash()
             << " generation=" << item->generation;
  auto size = item->blob->getSize();
  // TODO: Releasing this BlobPtr here can run arbitrary deleters which could,
  // in theory, try to reacquire the BlobCache's lock. The blob could be
  // scheduled for deletion in a deletion queue but then it's hard to ensure
  // that scheduling is noexcept. Instead, BlobPtr should be replaced with an
  // refcounted pointer that doesn't allow running custom deleters.
  state.items.erase(item->blob->getHash());
  state.totalSize -= size;
}

} // namespace eden
} // namespace facebook
