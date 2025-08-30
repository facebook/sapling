/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/IntrusiveList.h>
#include <folly/Synchronized.h>
#include <folly/container/F14Map.h>
#include <folly/synchronization/DistributedMutex.h>
#include <list>
#include <mutex>

#include "eden/fs/model/ObjectId.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {

enum class ObjectCacheFlavor { Simple, InterestHandle };

template <
    typename ObjectType,
    ObjectCacheFlavor Flavor,
    typename ObjectCacheStats>
class ObjectCache;

/**
 * Cache lookups return a ObjectInterestHandle which should be held as long as
 * the object remains interesting. See comments on ObjectCache for more
 * information on these.
 */
template <typename ObjectType, typename ObjectCacheStats>
class ObjectInterestHandle {
 public:
  ObjectInterestHandle() noexcept = default;

  ~ObjectInterestHandle() noexcept {
    reset();
  }

  ObjectInterestHandle(ObjectInterestHandle&& other) noexcept = default;
  ObjectInterestHandle& operator=(ObjectInterestHandle&& other) noexcept =
      default;

  /**
   * If this is a valid interest handle, and the object is still in cache,
   * return the corresponding object and move it to the back of the eviction
   * queue.
   *
   * Otherwise, return nullptr.
   */
  std::shared_ptr<const ObjectType> getObject() const;

  void reset() noexcept;

 private:
  ObjectInterestHandle(
      std::weak_ptr<ObjectCache<
          ObjectType,
          ObjectCacheFlavor::InterestHandle,
          ObjectCacheStats>> objectCache,
      ObjectId id,
      std::weak_ptr<const ObjectType> object,
      uint64_t generation) noexcept;

  std::weak_ptr<ObjectCache<
      ObjectType,
      ObjectCacheFlavor::InterestHandle,
      ObjectCacheStats>>
      objectCache_;

  // id_ is only accessed if ObjectCache_ is non-expired.
  ObjectId id_;

  // In the situation that the object exists even if it's been evicted, allow
  // retrieving it anyway.
  std::weak_ptr<const ObjectType> object_;

  // Only causes eviction if this matches the corresponding
  // CacheItem::generation.
  uint64_t cacheItemGeneration_{0};

  friend class ObjectCache<
      ObjectType,
      ObjectCacheFlavor::InterestHandle,
      ObjectCacheStats>;
};

/**
 * An in-memory LRU cache for loaded objects. It is parameterized by both a
 * maximum cache size and a minimum entry count. The cache tries to evict
 * entries when the total number of loaded objects exceeds the maximum cache
 * size, except that it always keeps the minimum entry count around.
 *
 * The intent of the minimum entry count is to avoid having to reload
 * frequently-accessed large objects when they are larger than the maximum cache
 * size.
 *
 * There are two flavors baked into this cache: Simple and InterestHandle.
 * Flavors should not be mixed! Only one flavor of methods should be used
 * per cache instance. The Simple flavor is a basic LRU cache. The
 * InterestHandle flavor introduces  a quicker lookup scheme for duplicate
 * lookups. The Interesthandle returned by insert and get can be used to lookup
 * the data directly if its needed again within a short period of time.
 * InterestHandles can also give the cache extra eviction hints. Interest
 * handles do not prevent entries from being evicted from the cache, but a lack
 * of InterestHandles for an object can mean it is evicted early.
 *
 * This class is not intended to be used directly, instead child classes should
 * be used that only allow clients to use one flavor of get and insert. See
 * BlobCache and TreeCache for examples of each flavor.
 *
 * It is safe to use this object from arbitrary threads.
 */
template <
    typename ObjectType,
    ObjectCacheFlavor Flavor,
    typename ObjectCacheStats>
class ObjectCache : public std::enable_shared_from_this<
                        ObjectCache<ObjectType, Flavor, ObjectCacheStats>> {
 public:
  using ObjectPtr = std::shared_ptr<const ObjectType>;

  enum class Interest {
    /**
     * Will return a object if it is cached, but not add a reference to it nor
     * move it to the back of the eviction queue.
     */
    UnlikelyNeededAgain,

    /**
     * If a object is cached, its reference count is incremented and a handle is
     * returned that, when dropped, releases the reference and evicts the item
     * from cache. Intended for satisfying a series of object reads from cache
     * until the inode is unloaded, after which the object can evicted from
     * cache, freeing space.
     */
    WantHandle,

    /**
     * If a object is cached, its reference count is incremented, but no
     * interest handle is returned. It is assumed to be worth caching until it
     * is naturally evicted.
     */
    LikelyNeededAgain,

    /**
     * This value is used if the item is not cached due to
     * blobcache:enable-in-memory-blob-caching being set to false
     */
    None,
  };

  struct GetResult {
    ObjectPtr object;
    ObjectInterestHandle<ObjectType, ObjectCacheStats> interestHandle;
  };

  struct Stats {
    size_t objectCount{0};
    size_t totalSizeInBytes{0};
    uint64_t hitCount{0};
    uint64_t missCount{0};
    uint64_t evictionCount{0};
    uint64_t dropCount{0};
  };

  static std::shared_ptr<ObjectCache<ObjectType, Flavor, ObjectCacheStats>>
  create(
      size_t maximumCacheSizeBytes,
      size_t minimumEntryCount,
      EdenStatsPtr stats);
  ~ObjectCache() {
    clear();
  }

  /**
   * If a object for the given id is in cache, return it. If the object is not
   * in cache, return nullptr (and an empty interest handle).
   *
   * If a object is returned and interest is WantHandle, then a movable handle
   * object is also returned. When the interest handle is destroyed, the cached
   * object may be evicted.
   *
   * After fetching a object, prefer calling getObject() on the returned
   * ObjectInterestHandle first. It can avoid some overhead or return a object
   * if it still exists in memory and the ObjectCache has evicted its reference.
   */
  template <ObjectCacheFlavor F = Flavor>
  typename std::enable_if_t<
      F == ObjectCacheFlavor::InterestHandle,
      typename ObjectCache<ObjectType, Flavor, ObjectCacheStats>::GetResult>
  getInterestHandle(
      const ObjectId& id,
      Interest interest = Interest::LikelyNeededAgain);

  /**
   * If a object for the given id is in cache, return it. If the object is not
   * in cache, return nullptr.
   */
  template <ObjectCacheFlavor F = Flavor>
  typename std::enable_if_t<
      F == ObjectCacheFlavor::Simple,
      typename ObjectCache<ObjectType, Flavor, ObjectCacheStats>::ObjectPtr>
  getSimple(const ObjectId& id);

  /**
   * Inserts a object into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted.
   *
   * Optionally returns an interest handle that, when dropped, evicts the
   * inserted object.
   */
  template <ObjectCacheFlavor F = Flavor>
  typename std::enable_if_t<
      F == ObjectCacheFlavor::InterestHandle,
      ObjectInterestHandle<ObjectType, ObjectCacheStats>>
  insertInterestHandle(
      ObjectId id,
      ObjectPtr object,
      Interest interest = Interest::LikelyNeededAgain);

  /**
   * Inserts a object into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted.
   */
  template <ObjectCacheFlavor F = Flavor>
  typename std::enable_if_t<F == ObjectCacheFlavor::Simple, void> insertSimple(
      ObjectId id,
      ObjectPtr object);

  /**
   * Returns true if the cache contains a object for the given id.
   */
  bool contains(const ObjectId& id) const;

  /**
   * Evicts everything from cache.
   */
  void clear();

  /**
   * Returns the memory footprint of the cache. This is meant to be used for
   * dynamic counter registration
   */
  size_t getTotalSizeBytes() const;

  /**
   * Returns the number of objects in the cache. This is meant to be used for
   * dynamic counter registration
   */
  size_t getObjectCount() const;

  /**
   * Return information about the current size of the cache and the total number
   * of hits and misses.
   */
  Stats getStats(const std::map<std::string, int64_t>& counters) const;

 protected:
  explicit ObjectCache(
      size_t maximumCacheSizeBytes,
      size_t minimumEntryCount,
      EdenStatsPtr stats);

  void invalidate(const ObjectId& id) noexcept;

 private:
  /*
   * TODO: This data structure could be implemented more efficiently. But since
   * most of the data will be held in the objects themselves and not in this
   * index, the overhead is not worrisome.
   *
   * But should we ever decide to optimize it, storing the array of CacheItem
   * nodes in a std::vector with indices to its siblings and to the next node
   * in the id chain would be more efficient, especially since the indices
   * could be smaller than a pointer.
   */

  struct CacheItem {
    // WARNING: leaves index unset. Since the items map and evictionQueue are
    // circular, initialization of index must happen after the CacheItem is
    // constructed.
    explicit CacheItem(ObjectId id, ObjectPtr b)
        : id{std::move(id)}, object{std::move(b)} {}

    // The folly::SafeIntrusiveListHook needs special handling to be
    // copied/moved, removing the move/copy constructor and assignment to
    // avoid unexpected copies/moves.
    CacheItem(CacheItem&&) = default;
    CacheItem(const CacheItem&) = delete;
    CacheItem& operator=(CacheItem&&) = default;
    CacheItem& operator=(const CacheItem&) = delete;

    ObjectId id;
    ObjectPtr object;
    folly::SafeIntrusiveListHook hook;

    /// Incremented on every LikelyNeededAgain or WantInterestHandle.
    /// Decremented on every dropInterestHandle. Evicted if it reaches zero.
    uint64_t referenceCount{0};

    /// Given a unique value upon allocation. Used to verify InterestHandle
    /// matches this specific item.
    uint64_t generation{std::numeric_limits<uint64_t>::max()};
  };

  struct State {
    explicit State(EdenStatsPtr stats) : stats{std::move(stats)} {}

    size_t totalSize{0};
    folly::F14NodeMap<ObjectId, CacheItem> items;

    /// Entries are evicted from the front of the queue.
    folly::CountedIntrusiveList<CacheItem, &CacheItem::hook> evictionQueue;

    EdenStatsPtr stats;
  };

  // for less typing
  using LockedState =
      typename folly::Synchronized<State, folly::DistributedMutex>::LockedPtr;

  /**
   * The "core" implementation for the method getInterestHandle().
   *
   * This method is not thread safe in any version of ObjectCache so it expects
   * to be only called by a single thread, meaning that a lock should be held
   * for the duration of the call.
   */
  template <ObjectCacheFlavor F = Flavor>
  typename std::enable_if_t<
      F == ObjectCacheFlavor::InterestHandle,
      typename ObjectCache<ObjectType, Flavor, ObjectCacheStats>::GetResult>
  getInterestHandleCore(
      LockedState& state,
      const ObjectId& id,
      Interest interest) noexcept;

  /**
   * The "core" implementation for the method insertInterestHandle().
   *
   * This method is not thread safe in any version of ObjectCache so it expects
   * to be only called by a single thread, meaning that a lock should be held
   * for the duration of the call.
   */
  template <ObjectCacheFlavor F = Flavor>
  typename std::enable_if_t<
      F == ObjectCacheFlavor::InterestHandle,
      ObjectInterestHandle<ObjectType, ObjectCacheStats>>
  insertInterestHandleCore(
      ObjectId id,
      ObjectPtr object,
      Interest interest,
      LockedState& state,
      uint64_t cacheItemGeneration,
      ObjectInterestHandle<ObjectType, ObjectCacheStats> interestHandle);

  struct PreProcessInterestHandleResult {
    ObjectInterestHandle<ObjectType, ObjectCacheStats> interestHandle;
    uint64_t cacheItemGeneration;
  };

  /**
   * Preprocess the interest handle for the given ID and object based on the
   * interest.
   * Returns a PreProcessInterestHandleResult object which contains the handle
   * we might need to insert or there is no need to insert when "ready" is True.
   *
   * We are doing this step in a separate function because this is a thread-safe
   * step that can be decoupled from the main insertion logic.
   */
  template <ObjectCacheFlavor F>
  typename std::enable_if_t<
      F == ObjectCacheFlavor::InterestHandle,
      PreProcessInterestHandleResult>
  preProcessInterestHandle(ObjectId id, ObjectPtr object, Interest interest);

  /**
   * If an object for the given id is in cache, return it. If the object is
   * not in cache, return nullptr (and an empty interest handle).
   *
   * Does not do anything related to interest handles.
   */
  CacheItem* getImpl(const ObjectId& id, State& state);

  /**
   * Inserts an object into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted. Returns the item inserted (or already in the cache if this is a
   * duplicate insert) and a boolean indicating if this item was freshly
   * inserted (returns false if this is a duplicate insert).
   *
   * Does not do anything related to InterestHandles
   */
  std::pair<CacheItem*, bool>
  insertImpl(ObjectId id, ObjectPtr object, State& state);

  void dropInterestHandle(const ObjectId& id, uint64_t generation) noexcept;

  void evictUntilFits(State& state) noexcept;
  void evictOne(State& state) noexcept;
  void evictItem(State&, const CacheItem& item) noexcept;

  const size_t maximumCacheSizeBytes_;
  const size_t minimumEntryCount_;
  folly::Synchronized<State, folly::DistributedMutex> state_;

  friend class ObjectInterestHandle<ObjectType, ObjectCacheStats>;
};

} // namespace facebook::eden

#include "eden/fs/store/ObjectCache-inl.h"
