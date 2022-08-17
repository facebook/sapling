/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <folly/container/F14Map.h>
#include <folly/synchronization/DistributedMutex.h>
#include <list>
#include <mutex>

#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {

enum class ObjectCacheFlavor { Simple, InterestHandle };

template <typename ObjectType, ObjectCacheFlavor Flavor>
class ObjectCache;

/**
 * Cache lookups return a ObjectInterestHandle which should be held as long as
 * the object remains interesting. See comments on ObjectCache for more
 * information on these.
 */
template <typename ObjectType>
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
      std::weak_ptr<ObjectCache<ObjectType, ObjectCacheFlavor::InterestHandle>>
          objectCache,
      const ObjectId& hash,
      std::weak_ptr<const ObjectType> object,
      uint64_t generation) noexcept;

  std::weak_ptr<ObjectCache<ObjectType, ObjectCacheFlavor::InterestHandle>>
      objectCache_;

  // hash_ is only accessed if ObjectCache_ is non-expired.
  ObjectId hash_;

  // In the situation that the object exists even if it's been evicted, allow
  // retrieving it anyway.
  std::weak_ptr<const ObjectType> object_;

  // Only causes eviction if this matches the corresponding
  // CacheItem::generation.
  uint64_t cacheItemGeneration_{0};

  friend class ObjectCache<ObjectType, ObjectCacheFlavor::InterestHandle>;
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
 * InterestHandles can also give the cache exta eviction hints. Interest
 * handles do not prevent entries from being evicted from the cache, but a lack
 * of InterestHandles for an object can mean it is evicted early.
 *
 * This class is not intended to be used directly, instead child classes should
 * be used that only allow clients to use one flavor of get and insert. See
 * BlobCache and TreeCache for examples of each flavor.
 *
 * It is safe to use this object from arbitrary threads.
 */
template <typename ObjectType, ObjectCacheFlavor Flavor>
class ObjectCache
    : public std::enable_shared_from_this<ObjectCache<ObjectType, Flavor>> {
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
  };

  struct GetResult {
    ObjectPtr object;
    ObjectInterestHandle<ObjectType> interestHandle;
  };

  struct Stats {
    size_t objectCount{0};
    size_t totalSizeInBytes{0};
    uint64_t hitCount{0};
    uint64_t missCount{0};
    uint64_t evictionCount{0};
    uint64_t dropCount{0};
  };

  static std::shared_ptr<ObjectCache<ObjectType, Flavor>> create(
      size_t maximumCacheSizeBytes,
      size_t minimumEntryCount);
  ~ObjectCache() {}

  /**
   * If a object for the given hash is in cache, return it. If the object is not
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
      typename ObjectCache<ObjectType, Flavor>::GetResult>
  getInterestHandle(
      const ObjectId& hash,
      Interest interest = Interest::LikelyNeededAgain);

  /**
   * If a object for the given hash is in cache, return it. If the object is not
   * in cache, return nullptr.
   */
  template <ObjectCacheFlavor F = Flavor>
  typename std::enable_if_t<
      F == ObjectCacheFlavor::Simple,
      typename ObjectCache<ObjectType, Flavor>::ObjectPtr>
  getSimple(const ObjectId& hash);

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
      ObjectInterestHandle<ObjectType>>
  insertInterestHandle(
      ObjectPtr object,
      Interest interest = Interest::LikelyNeededAgain);

  /**
   * Inserts a object into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted.
   */
  template <ObjectCacheFlavor F = Flavor>
  typename std::enable_if_t<F == ObjectCacheFlavor::Simple, void> insertSimple(
      ObjectPtr object);

  /**
   * Returns true if the cache contains a object for the given hash.
   */
  bool contains(const ObjectId& hash) const;

  /**
   * Evicts everything from cache.
   */
  void clear();

  /**
   * Return information about the current size of the cache and the total number
   * of hits and misses.
   */
  Stats getStats() const;

 protected:
  explicit ObjectCache(size_t maximumCacheSizeBytes, size_t minimumEntryCount);

 private:
  /*
   * TODO: This data structure could be implemented more efficiently. But since
   * most of the data will be held in the objects themselves and not in this
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
    explicit CacheItem(ObjectPtr b) : object{std::move(b)} {}

    ObjectPtr object;

    typename std::list<CacheItem*>::iterator index;

    /// Incremented on every LikelyNeededAgain or WantInterestHandle.
    /// Decremented on every dropInterestHandle. Evicted if it reaches zero.
    uint64_t referenceCount{0};

    /// Given a unique value upon allocation. Used to verify InterestHandle
    /// matches this specific item.
    uint64_t generation{std::numeric_limits<uint64_t>::max()};
  };

  struct State {
    size_t totalSize{0};
    // A F14FastMap cannot be used as it moves elements on insertion/removal,
    // but the evictionQueue below relies on moves not occuring.
    folly::F14NodeMap<ObjectId, CacheItem> items;

    /// Entries are evicted from the front of the queue.
    std::list<CacheItem*> evictionQueue;

    uint64_t hitCount{0};
    uint64_t missCount{0};
    uint64_t evictionCount{0};
    uint64_t dropCount{0};
  };

  /**
   * If an object for the given hash is in cache, return it. If the object is
   * not in cache, return nullptr (and an empty interest handle).
   *
   * Does not do anything related to interest handles.
   */
  CacheItem* getImpl(const ObjectId& hash, State& state);

  /**
   * Inserts an object into the cache for future lookup. If the new total size
   * exceeds the maximum cache size and the minimum entry count, old entries are
   * evicted. Returns the item inserted (or already in the cache if this is a
   * duplicate insert) and a boolean indicating if this item was freshly
   * inserted (returns false if this is a duplicate insert).
   *
   * Does not do anything related to InterestHandles
   */
  std::pair<CacheItem*, bool> insertImpl(ObjectPtr object, State& state);

  void dropInterestHandle(const ObjectId& hash, uint64_t generation) noexcept;

  void evictUntilFits(State& state) noexcept;
  void evictOne(State& state) noexcept;
  void evictItem(State&, CacheItem* item) noexcept;

  const size_t maximumCacheSizeBytes_;
  const size_t minimumEntryCount_;
  folly::Synchronized<State, folly::DistributedMutex> state_;

  friend class ObjectInterestHandle<ObjectType>;
};

} // namespace facebook::eden

#include "eden/fs/store/ObjectCache-inl.h"
