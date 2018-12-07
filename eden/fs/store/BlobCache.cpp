/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "BlobCache.h"
#include <folly/MapUtil.h>
#include <folly/logging/xlog.h>
#include "eden/fs/model/Blob.h"

namespace facebook {
namespace eden {

BlobInterestHandle::BlobInterestHandle(std::weak_ptr<const Blob> blob)
    : blob_{std::move(blob)} {
  // No need to initialize hash_ because blobCache_ is unset.
}

BlobInterestHandle::BlobInterestHandle(
    std::weak_ptr<BlobCache> blobCache,
    const Hash& hash,
    std::weak_ptr<const Blob> blob)
    : blobCache_{std::move(blobCache)}, hash_{hash}, blob_{std::move(blob)} {}

void BlobInterestHandle::reset() noexcept {
  if (auto blobCache = blobCache_.lock()) {
    blobCache->dropInterestHandle(hash_);
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
  // Acquires BlobCache's lock upon destruction by calling dropInterestHandle,
  // so ensure that, if an exception is thrown below, the ~BlobInterestHandle
  // runs after the lock is released.
  BlobInterestHandle interestHandle;

  auto state = state_.wlock();

  auto* item = folly::get_ptr(state->items, hash);
  if (!item) {
    ++state->missCount;
    return GetResult{};
  }

  switch (interest) {
    case Interest::UnlikelyNeededAgain:
      interestHandle.blob_ = item->blob;
      break;
    case Interest::WantHandle:
      interestHandle = BlobInterestHandle{shared_from_this(), hash, item->blob};
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
  // Acquires BlobCache's lock upon destruction by calling dropInterestHandle,
  // so ensure that, if an exception is thrown below, the ~BlobInterestHandle
  // runs after the lock is released.
  BlobInterestHandle interestHandle;

  auto hash = blob->getHash();
  auto size = blob->getSize();

  if (interest == Interest::WantHandle) {
    // This can throw, so do it before inserting into items.
    interestHandle = BlobInterestHandle{shared_from_this(), hash, blob};
  } else {
    interestHandle.blob_ = blob;
  }

  auto state = state_.wlock();
  auto [iter, inserted] = state->items.try_emplace(hash, std::move(blob));
  // noexcept from here until `try`
  switch (interest) {
    case Interest::UnlikelyNeededAgain:
      break;
    case Interest::WantHandle:
    case Interest::LikelyNeededAgain:
      ++iter->second.referenceCount;
      break;
  }
  if (inserted) {
    auto* itemPtr = &iter->second;
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
    state->evictionQueue.splice(
        state->evictionQueue.end(), state->evictionQueue, iter->second.index);
  }
  return interestHandle;
}

bool BlobCache::contains(const Hash& hash) const {
  auto state = state_.rlock();
  return 1 == state->items.count(hash);
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

void BlobCache::dropInterestHandle(const Hash& hash) noexcept {
  auto state = state_.wlock();

  auto* item = folly::get_ptr(state->items, hash);
  if (!item) {
    // Cached item already evicted.
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
