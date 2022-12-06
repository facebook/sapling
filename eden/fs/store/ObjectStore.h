/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <folly/container/EvictingCacheMap.h>
#include <memory>
#include <unordered_map>

#include <folly/logging/xlog.h>
#include "eden/common/utils/ProcessNameCache.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/StructuredLogger.h"

namespace facebook::eden {

class BackingStore;
class Blob;
class LocalStore;
class Tree;
enum class ObjectComparison : uint8_t;

struct PidFetchCounts {
  folly::Synchronized<std::unordered_map<pid_t, uint64_t>> map_;

  uint64_t recordProcessFetch(pid_t pid) {
    auto map_lock = map_.wlock();
    auto fetch_count = (*map_lock)[pid]++;
    return fetch_count;
  }

  void clear() {
    map_.wlock()->clear();
  }

  uint64_t getCountByPid(pid_t pid) {
    auto rl = map_.rlock();
    auto fetch_count = rl->find(pid);
    if (fetch_count != rl->end()) {
      return fetch_count->second;
    } else {
      return 0;
    }
  }
};

/**
 * ObjectStore is a content-addressed store for eden object data.
 *
 * The ObjectStore class itself is primarily a wrapper around two other
 * underlying storage types:
 * - LocalStore, which caches object data locally in a RocksDB instance
 * - BackingStore, which represents the authoritative source for the object
 *   data.  The BackingStore is generally more expensive to query for object
 *   data, and may not be available during offline operation.
 */
class ObjectStore : public IObjectStore,
                    public RootIdCodec,
                    public ObjectIdCodec,
                    public std::enable_shared_from_this<ObjectStore> {
 public:
  static std::shared_ptr<ObjectStore> create(
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<BackingStore> backingStore,
      std::shared_ptr<TreeCache> treeCache,
      std::shared_ptr<EdenStats> stats,
      std::shared_ptr<ProcessNameCache> processNameCache,
      std::shared_ptr<StructuredLogger> structuredLogger,
      std::shared_ptr<const EdenConfig> edenConfig,
      CaseSensitivity caseSensitive);
  ~ObjectStore() override;

  /**
   * When pid of fetchContext is available, this function updates
   * pidFetchCounts_. If the current process needs to be logged as
   * a fetch-heavy process, it sends a FetchHeavy event to Scuba.
   */
  void updateProcessFetch(const ObjectFetchContext& fetchContext) const;

  /**
   * send a FetchHeavy log event to Scuba. If either processNameCache_
   * or structuredLogger_ is nullptr, this function does nothing.
   */
  void sendFetchHeavyEvent(pid_t pid, uint64_t fetch_count) const;

  /**
   * Check fetch count of the process using this fetchContext before using
   * the fetchContext in BackingStore. if fetchHeavyThreshold in edenConfig_ is
   * exceeded, deprioritize the fetchContext by 1.
   *
   * Note: Normally, one fetchContext is created for only one fetch request,
   * so deprioritize() should only be called once by one thread, but that is
   * not strictly guaranteed. See comments before deprioritize() for more
   * information
   */
  void deprioritizeWhenFetchHeavy(ObjectFetchContext& context) const;

  /**
   * Each BackingStore implementation defines its interpretation of root IDs.
   * This function gives the BackingStore a chance to parse and canonicalize the
   * root ID at API boundaries such as Thrift.
   */
  RootId parseRootId(folly::StringPiece rootId) override;

  /**
   * Each BackingStore defines the meaning and encoding of its root ID. Give it
   * the chance to render a root ID to Thrift.
   */
  std::string renderRootId(const RootId& rootId) override;

  /**
   * Each BackingStore implementation defines its interpretation of object IDs.
   * This function gives the BackingStore a chance to parse and canonicalize the
   * object ID at API boundaries such as Thrift.
   */
  ObjectId parseObjectId(folly::StringPiece objectId) override;

  /**
   * Each BackingStore defines the meaning and encoding of its object ID. Give
   * it the chance to render a object ID to Thrift.
   */
  std::string renderObjectId(const ObjectId& objectId) override;

  /**
   * Get a root Tree.
   *
   * This returns a Future object that will produce the root Tree when it is
   * ready.  It may result in a std::domain_error if the specified commit ID
   * does not exist, or possibly other exceptions on error.
   */
  ImmediateFuture<std::shared_ptr<const Tree>> getRootTree(
      const RootId& rootId,
      const ObjectFetchContextPtr& context) const override;

  /**
   * Get a TreeEntry by ID
   *
   * This returns a Future object that will produce the TreeEntry when it is
   * ready. It may result in a std::domain_error if the specified tree ID does
   * not exist, or possibly other exceptions on error.
   */
  ImmediateFuture<std::shared_ptr<TreeEntry>> getTreeEntryForObjectId(
      const ObjectId& objectId,
      TreeEntryType treeEntryType,
      const ObjectFetchContextPtr& context) const;

  /**
   * Get a Tree by ID.
   *
   * This returns an ImmediateFuture object that will produce the Tree when it
   * is ready.  It may result in a std::domain_error if the specified tree ID
   * does not exist, or possibly other exceptions on error.
   */
  ImmediateFuture<std::shared_ptr<const Tree>> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const override;

  /**
   * Prefetch all the blobs represented by the HashRange.
   *
   * The caller is responsible for making sure that the HashRange stays valid
   * for as long as the returned ImmediateFuture.
   */
  ImmediateFuture<folly::Unit> prefetchBlobs(
      ObjectIdRange ids,
      const ObjectFetchContextPtr& context) const override;

  /**
   * Get a Blob by ID.
   *
   * This returns a Future object that will produce the Blob when it is ready.
   * It may result in a std::domain_error if the specified blob ID does not
   * exist, or possibly other exceptions on error.
   */
  ImmediateFuture<std::shared_ptr<const Blob>> getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const override;

  /**
   * Get metadata about a Blob.
   *
   * This returns an ImmediateFuture object that will produce the BlobMetadata
   * when it is ready.  It may result in a std::domain_error if the specified
   * blob does not exist, or possibly other exceptions on error.
   */
  ImmediateFuture<BlobMetadata> getBlobMetadata(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const;

  /**
   * Returns the size of the contents of the blob with the given ID.
   */
  ImmediateFuture<uint64_t> getBlobSize(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const;

  /**
   * Returns the SHA-1 hash of the contents of the blob with the given ID.
   */
  ImmediateFuture<Hash20> getBlobSha1(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const;

  /**
   * Compares the objects.
   *
   * Returns true when the objects either refer to the same object
   * (areObjectsKnownIdentical), or if their content are the same.
   */
  ImmediateFuture<bool> areBlobsEqual(
      const ObjectId& one,
      const ObjectId& two,
      const ObjectFetchContextPtr& context) const;

  /**
   * Get the LocalStore used by this ObjectStore
   */
  const std::shared_ptr<LocalStore>& getLocalStore() const {
    return localStore_;
  }

  /**
   * Get the BackingStore used by this ObjectStore
   */
  const std::shared_ptr<BackingStore>& getBackingStore() const {
    return backingStore_;
  }

  /**
   * Convenience wrapper around BackingStore::compareObjectsById.  See
   * `BackingStorecompareObjectsById`'s documentation for more details.
   */
  ObjectComparison compareObjectsById(const ObjectId& one, const ObjectId& two)
      const;

  /**
   * Convenience wrapper around compareObjectsById for the common case that the
   * caller wants to know if two IDs refer to the same object.
   *
   * Returns true only if the objects are known identical. If they are known
   * different or if the BackingStore can't determine if they're identical,
   * returns false.
   *
   * This function is used to short-circuit deep comparisons of objects.
   */
  bool areObjectsKnownIdentical(const ObjectId& one, const ObjectId& two) const;

  folly::Synchronized<std::unordered_map<pid_t, uint64_t>>& getPidFetches() {
    return pidFetchCounts_->map_;
  }

  void clearFetchCounts() {
    pidFetchCounts_->clear();
  }

 private:
  // Forbidden constructor. Use create().
  ObjectStore(
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<BackingStore> backingStore,
      std::shared_ptr<TreeCache> treeCache,
      std::shared_ptr<EdenStats> stats,
      std::shared_ptr<ProcessNameCache> processNameCache,
      std::shared_ptr<StructuredLogger> structuredLogger,
      std::shared_ptr<const EdenConfig> edenConfig,
      CaseSensitivity caseSensitive);
  // Forbidden copy constructor and assignment operator
  ObjectStore(ObjectStore const&) = delete;
  ObjectStore& operator=(ObjectStore const&) = delete;

  static constexpr size_t kCacheSize = 1000000;

  /**
   * During status and checkout, it's common to look up the SHA-1 for a given
   * blob ID. To avoid needing to hit RocksDB, keep a bounded in-memory cache of
   * the sizes and SHA-1s of blobs we've seen. Each node is somewhere around 50
   * bytes (20+28 + LRU overhead) and we store kMetadataCacheSize entries, which
   * EvictingCacheMap divides in two for some reason. At the time of this
   * comment, EvictingCacheMap does not store its nodes densely, so there may
   * also be some jemalloc tracking overhead and some internal fragmentation
   * depending on whether the node fits cleanly into one of jemalloc's size
   * classes.
   *
   * TODO: It never makes sense to rlock an LRU cache, since cache hits mutate
   * the data structure. Thus, should we use a more appropriate type of lock?
   */
  mutable folly::Synchronized<folly::EvictingCacheMap<ObjectId, BlobMetadata>>
      metadataCache_;

  /**
   * During glob, we need to read a lot of trees, but we avoid loading inodes,
   * so this means we go to RocksDB for each tree read. To avoid needing to hit
   * RocksDB, keep a bounded in-memory cache of the trees we've seen.
   * This cache will also be read from the first time we load a tree inode.
   * This cache is shared accross all object stores, and has a fixed memory
   * size. (The one size limit violation is if there are very large trees,
   * the cache is allowed to retain a small fixed number of these in cache, and
   * violate the fixed size. This generally, should be rare as no trees should
   * approach the size limit of the cache.)
   */
  const std::shared_ptr<TreeCache> treeCache_;

  /*
   * The LocalStore.
   *
   * Multiple ObjectStores (for different mount points) may share the same
   * LocalStore.
   */
  std::shared_ptr<LocalStore> localStore_;
  /*
   * The BackingStore.
   *
   * Multiple ObjectStores may share the same BackingStore.
   */
  std::shared_ptr<BackingStore> backingStore_;

  std::shared_ptr<EdenStats> const stats_;

  /* number of fetches for each process collected
   * from the beginning of the eden daemon progress */
  std::unique_ptr<PidFetchCounts> pidFetchCounts_;

  /* process name cache and structured logger used for
   * sending fetch heavy events, set to nullptr if not
   * initialized by create()
   */
  std::shared_ptr<ProcessNameCache> processNameCache_;
  std::shared_ptr<StructuredLogger> structuredLogger_;
  std::shared_ptr<const EdenConfig> edenConfig_;

  // Is this ObjectStore case sensitive? This only matters for methods returning
  // Tree.
  CaseSensitivity caseSensitive_;
};

} // namespace facebook::eden
