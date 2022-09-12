/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/MapUtil.h>
#include <folly/logging/xlog.h>
#include <utility>

#include "eden/fs/store/ObjectCache.h"
#include "eden/fs/utils/IDGen.h"

namespace facebook::eden {

template <typename ObjectType>
ObjectInterestHandle<ObjectType>::ObjectInterestHandle(
    std::weak_ptr<ObjectCache<ObjectType, ObjectCacheFlavor::InterestHandle>>
        objectCache,
    const ObjectId& hash,
    std::weak_ptr<const ObjectType> object,
    uint64_t generation) noexcept
    : objectCache_{std::move(objectCache)},
      hash_{hash},
      object_{std::move(object)},
      cacheItemGeneration_{generation} {}

template <typename ObjectType>
void ObjectInterestHandle<ObjectType>::reset() noexcept {
  if (auto objectCache = objectCache_.lock()) {
    objectCache->dropInterestHandle(hash_, cacheItemGeneration_);
  }
  objectCache_.reset();
}

template <typename ObjectType>
std::shared_ptr<const ObjectType> ObjectInterestHandle<ObjectType>::getObject()
    const {
  auto objectCache = objectCache_.lock();
  if (objectCache) {
    // UnlikelyNeededAgain because there's no need to create a new interest
    // handle nor bump the refcount.
    auto object =
        objectCache
            ->getInterestHandle(
                hash_,
                ObjectCache<ObjectType, ObjectCacheFlavor::InterestHandle>::
                    Interest::UnlikelyNeededAgain)
            .object;
    if (object) {
      return object;
    }
  }

  // If the object is no longer in cache, at least see if it's still in memory.
  return object_.lock();
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
std::shared_ptr<ObjectCache<ObjectType, Flavor>>
ObjectCache<ObjectType, Flavor>::create(
    size_t maximumCacheSizeBytes,
    size_t minimumEntryCount) {
  // Allow make_shared with private constructor.
  struct OC : ObjectCache<ObjectType, Flavor> {
    OC(size_t x, size_t y) : ObjectCache<ObjectType, Flavor>{x, y} {}
  };
  return std::make_shared<OC>(maximumCacheSizeBytes, minimumEntryCount);
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
ObjectCache<ObjectType, Flavor>::ObjectCache(
    size_t maximumCacheSizeBytes,
    size_t minimumEntryCount)
    : maximumCacheSizeBytes_{maximumCacheSizeBytes},
      minimumEntryCount_{minimumEntryCount} {}

template <typename ObjectType, ObjectCacheFlavor Flavor>
template <ObjectCacheFlavor F>
typename std::enable_if_t<
    F == ObjectCacheFlavor::InterestHandle,
    typename ObjectCache<ObjectType, Flavor>::GetResult>
ObjectCache<ObjectType, Flavor>::getInterestHandle(
    const ObjectId& hash,
    Interest interest) {
  XLOG(DBG6) << "BlobCache::getInterestHandle " << hash;
  // Acquires ObjectCache's lock upon destruction by calling dropInterestHandle,
  // so ensure that, if an exception is thrown below, the ~ObjectInterestHandle
  // runs after the lock is released.
  ObjectInterestHandle<ObjectType> interestHandle;

  auto state = state_.lock();

  auto item = getImpl(hash, *state);
  if (!item) {
    return GetResult{};
  }

  switch (interest) {
    case Interest::UnlikelyNeededAgain:
      interestHandle.object_ = item->object;
      break;
    case Interest::WantHandle:
      interestHandle = ObjectInterestHandle<ObjectType>{
          this->shared_from_this(), hash, item->object, item->generation};
      ++item->referenceCount;
      break;
    case Interest::LikelyNeededAgain:
      interestHandle.object_ = item->object;
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

  return GetResult{item->object, std::move(interestHandle)};
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
template <ObjectCacheFlavor F>
typename std::enable_if_t<
    F == ObjectCacheFlavor::Simple,
    typename ObjectCache<ObjectType, Flavor>::ObjectPtr>
ObjectCache<ObjectType, Flavor>::getSimple(const ObjectId& hash) {
  XLOG(DBG6) << "BlobCache::getSimple " << hash;
  auto state = state_.lock();

  if (auto item = getImpl(hash, *state)) {
    return item->object;
  }
  return nullptr;
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
typename ObjectCache<ObjectType, Flavor>::CacheItem*
ObjectCache<ObjectType, Flavor>::getImpl(const ObjectId& hash, State& state) {
  XLOG(DBG6) << "ObjectCache::getImpl " << hash;
  auto* item = folly::get_ptr(state.items, hash);
  if (!item) {
    XLOG(DBG6) << "ObjectCache::getImpl missed";
    ++state.missCount;

  } else {
    XLOG(DBG6) << "ObjectCache::getImpl hit";

    // TODO: Should we avoid promoting if interest is UnlikelyNeededAgain?
    // For now, we'll try not to be too clever.
    state.evictionQueue.splice(
        state.evictionQueue.end(), state.evictionQueue, item->index);
    ++state.hitCount;
  }

  return item;
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
template <ObjectCacheFlavor F>
typename std::enable_if_t<
    F == ObjectCacheFlavor::InterestHandle,
    ObjectInterestHandle<ObjectType>>
ObjectCache<ObjectType, Flavor>::insertInterestHandle(
    ObjectPtr object,
    Interest interest) {
  XLOG(DBG6) << "ObjectCache::insertInterestHandle " << object->getHash();
  // Acquires ObjectCache's lock upon destruction by calling dropInterestHandle,
  // so ensure that, if an exception is thrown below, the ~ObjectInterestHandle
  // runs after the lock is released.
  ObjectInterestHandle<ObjectType> interestHandle{};

  auto cacheItemGeneration = generateUniqueID();

  if (interest == Interest::WantHandle) {
    // This can throw, so do it before inserting into items.
    interestHandle = ObjectInterestHandle<ObjectType>{
        this->shared_from_this(),
        object->getHash(),
        object,
        cacheItemGeneration};
  } else {
    interestHandle.object_ = object;
  }

  XLOG(DBG6) << "  creating entry with generation=" << cacheItemGeneration;

  auto state = state_.lock();
  auto [item, inserted] = insertImpl(std::move(object), *state);
  switch (interest) {
    case Interest::UnlikelyNeededAgain:
      break;
    case Interest::WantHandle:
    case Interest::LikelyNeededAgain:
      ++item->referenceCount;
      break;
  }
  if (inserted) { // new entry we need to set the generation number
    item->generation = cacheItemGeneration;
  } else {
    XLOG(DBG6) << "duplicate entry, using generation " << item->generation;
    // Inserting duplicate entry - use its generation.
    interestHandle.cacheItemGeneration_ = item->generation;
    // note we can skip eviction here because we didn't insert anything new,
    // so the cache size has not changed as a result of this operation.
    return interestHandle;
  }
  return interestHandle;
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
template <ObjectCacheFlavor F>
typename std::enable_if_t<F == ObjectCacheFlavor::Simple, void>
ObjectCache<ObjectType, Flavor>::insertSimple(
    ObjectCache<ObjectType, Flavor>::ObjectPtr object) {
  XLOG(DBG6) << "ObjectCache::insertSimple " << object->getHash();
  auto state = state_.lock();
  insertImpl(std::move(object), *state);
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
std::pair<typename ObjectCache<ObjectType, Flavor>::CacheItem*, bool>
ObjectCache<ObjectType, Flavor>::insertImpl(ObjectPtr object, State& state) {
  XLOG(DBG6) << "ObjectCache::insertImpl " << object->getHash();

  auto hash = object->getHash();
  auto size = object->getSizeBytes();

  // the following should be no except

  auto [iter, inserted] =
      state.items.try_emplace(std::move(hash), std::move(object));

  auto* itemPtr = &iter->second;
  if (inserted) {
    try {
      state.evictionQueue.push_back(itemPtr);
    } catch (const std::exception&) {
      state.items.erase(iter);
      throw;
    }
    iter->second.index = std::prev(state.evictionQueue.end());
    state.totalSize += size;
    evictUntilFits(state);
  } else {
    state.evictionQueue.splice(
        state.evictionQueue.end(), state.evictionQueue, itemPtr->index);
  }
  return std::make_pair(itemPtr, inserted);
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
bool ObjectCache<ObjectType, Flavor>::contains(const ObjectId& hash) const {
  auto state = state_.lock();
  return 1 == state->items.count(hash);
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
void ObjectCache<ObjectType, Flavor>::clear() {
  XLOG(DBG6) << "ObjectCache::clear";
  auto state = state_.lock();
  state->totalSize = 0;
  state->items.clear();
  state->evictionQueue.clear();
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
typename ObjectCache<ObjectType, Flavor>::Stats
ObjectCache<ObjectType, Flavor>::getStats() const {
  auto state = state_.lock();
  Stats stats;
  stats.objectCount = state->items.size();
  stats.totalSizeInBytes = state->totalSize;
  stats.hitCount = state->hitCount;
  stats.missCount = state->missCount;
  stats.evictionCount = state->evictionCount;
  stats.dropCount = state->dropCount;
  return stats;
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
void ObjectCache<ObjectType, Flavor>::dropInterestHandle(
    const ObjectId& hash,
    uint64_t generation) noexcept {
  XLOG(DBG6) << "dropInterestHandle " << hash << " generation=" << generation;
  auto state = state_.lock();

  auto* item = folly::get_ptr(state->items, hash);
  if (!item) {
    // Cached item already evicted.
    return;
  }

  if (generation != item->generation) {
    // Item was evicted and re-added between creating and dropping the
    // interest handle.
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

template <typename ObjectType, ObjectCacheFlavor Flavor>
void ObjectCache<ObjectType, Flavor>::evictUntilFits(State& state) noexcept {
  XLOG(DBG6) << "ObjectCache::evictUntilFits "
             << "state.totalSize=" << state.totalSize
             << ", maximumCacheSizeBytes_=" << maximumCacheSizeBytes_
             << ", evictionQueue.size()=" << state.evictionQueue.size()
             << ", minimumEntryCount_=" << minimumEntryCount_;
  while (state.totalSize > maximumCacheSizeBytes_ &&
         state.evictionQueue.size() > minimumEntryCount_) {
    evictOne(state);
  }
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
void ObjectCache<ObjectType, Flavor>::evictOne(State& state) noexcept {
  CacheItem* front = state.evictionQueue.front();
  state.evictionQueue.pop_front();
  ++state.evictionCount;
  evictItem(state, front);
}

template <typename ObjectType, ObjectCacheFlavor Flavor>
void ObjectCache<ObjectType, Flavor>::evictItem(
    State& state,
    CacheItem* item) noexcept {
  XLOG(DBG6) << "ObjectCache::evictItem "
             << "evicting " << item->object->getHash()
             << " generation=" << item->generation;
  auto size = item->object->getSizeBytes();
  // TODO: Releasing this ObjectPtr here can run arbitrary deleters which
  // could, in theory, try to reacquire the ObjectCache's lock. The object
  // could be scheduled for deletion in a deletion queue but then it's hard to
  // ensure that scheduling is noexcept. Instead, ObjectPtr should be replaced
  // with an refcounted pointer that doesn't allow running custom deleters.
  state.items.erase(item->object->getHash());
  state.totalSize -= size;
}
} // namespace facebook::eden
