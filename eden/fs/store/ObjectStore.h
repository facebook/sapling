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
#include <gtest/gtest_prod.h>

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/model/BlobAuxData.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

class Blob;
class EdenConfig;
class EdenStats;
class ProcessInfoCache;
class StructuredLogger;
class TreeCache;
class LocalStore;
enum class ObjectComparison : uint8_t;

using EdenStatsPtr = RefPtr<EdenStats>;

struct PidFetchCounts {
  folly::Synchronized<std::unordered_map<ProcessId, uint64_t>> map_;

  uint64_t recordProcessFetch(ProcessId pid) {
    auto map_lock = map_.wlock();
    auto fetch_count = (*map_lock)[pid]++;
    return fetch_count;
  }

  void clear() {
    map_.wlock()->clear();
  }

  uint64_t getCountByPid(ProcessId pid) {
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
 * - An in memory cache, for fast repeated queries,
 * - BackingStore, which represents the authoritative source for the object
 *   data.  The BackingStore is generally more expensive to query for object
 *   data, and may not be available during offline operation.
 *
 * ObjectStore also takes care of updating various stats and counters.
 */
class ObjectStore : public IObjectStore,
                    public RootIdCodec,
                    public ObjectIdCodec,
                    public std::enable_shared_from_this<ObjectStore> {
 public:
  static std::shared_ptr<ObjectStore> create(
      std::shared_ptr<BackingStore> backingStore,
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<TreeCache> treeCache,
      EdenStatsPtr stats,
      std::shared_ptr<ProcessInfoCache> processInfoCache,
      std::shared_ptr<StructuredLogger> structuredLogger,
      std::shared_ptr<const EdenConfig> edenConfig,
      bool windowsSymlinksEnabled,
      CaseSensitivity caseSensitive);
  ~ObjectStore() override;

  /**
   * When pid of fetchContext is available, this function updates
   * pidFetchCounts_. If the current process needs to be logged as
   * a fetch-heavy process, it sends a FetchHeavy event to Scuba.
   */
  void updateProcessFetch(const ObjectFetchContext& fetchContext) const;

  /**
   * send a FetchHeavy log event to Scuba. If either processInfoCache_
   * or structuredLogger_ is nullptr, this function does nothing.
   */
  void sendFetchHeavyEvent(ProcessId pid, uint64_t fetch_count) const;

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
  ImmediateFuture<GetRootTreeResult> getRootTree(
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
  ImmediateFuture<TreePtr> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const override;

  /**
   * Get aux data about a tree.
   *
   * This returns an ImmediateFuture object that will produce the TreeAuxData
   * when it is ready.  It may result in a std::domain_error if the specified
   * tree does not exist, or possibly other exceptions on error.
   */
  ImmediateFuture<TreeAuxData> getTreeAuxData(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const;

  /**
   * Get aux data about a Tree from EdenFS's in memory TreeAuxData cache.
   *
   * This returns an ImmediateFuture object that will produce the TreeAuxData
   * when it is ready.  It may result in a std::domain_error if the specified
   * tree does not exist, or possibly other exceptions on error.
   */
  std::optional<TreeAuxData> getTreeAuxDataFromInMemoryCache(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const;

  /**
   * Returns the DigestHash hash of the contents of the tree with the given ID.
   */
  ImmediateFuture<Hash32> getTreeDigestHash(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const;

  /**
   * Returns the digest size of the tree with the given ID.
   */
  ImmediateFuture<uint64_t> getTreeDigestSize(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const;

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
   * Get aux data about a Blob.
   *
   * This returns an ImmediateFuture object that will produce the BlobAuxData
   * when it is ready.  It may result in a std::domain_error if the specified
   * blob does not exist, or possibly other exceptions on error.
   */
  ImmediateFuture<BlobAuxData> getBlobAuxData(
      const ObjectId& id,
      const ObjectFetchContextPtr& context,
      bool blake3Needed = false) const;

  /**
   * Get aux data about a Blob from EdenFS's in memory BlobAuxData cache.
   *
   * This returns an ImmediateFuture object that will produce the BlobAuxData
   * when it is ready.  It may result in a std::domain_error if the specified
   * blob does not exist, or possibly other exceptions on error.
   */
  std::optional<BlobAuxData> getBlobAuxDataFromInMemoryCache(
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
   * Returns the Blake3 hash of the contents of the blob with the given ID.
   */
  ImmediateFuture<Hash32> getBlobBlake3(
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
   * Get file paths matching the given globs
   */
  ImmediateFuture<BackingStore::GetGlobFilesResult> getGlobFiles(
      const RootId& id,
      const std::vector<std::string>& globs,
      const std::vector<std::string>& prefixes,
      const ObjectFetchContextPtr& context) const;

  /**
   * Get the BackingStore used by this ObjectStore
   */
  const std::shared_ptr<BackingStore>& getBackingStore() const {
    return backingStore_;
  }

  /**
   * Get the TreeCache used by this ObjectStore
   */
  const std::shared_ptr<TreeCache>& getTreeCache() const {
    return treeCache_;
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

  void workingCopyParentHint(const RootId& parent) {
    backingStore_->workingCopyParentHint(parent);
  }

  folly::Synchronized<std::unordered_map<ProcessId, uint64_t>>&
  getPidFetches() {
    return pidFetchCounts_->map_;
  }

  void clearFetchCounts() {
    pidFetchCounts_->clear();
  }

  bool getWindowsSymlinksEnabled() const {
    return windowsSymlinksEnabled_;
  }

 private:
  FRIEND_TEST(ObjectStoreTest, caching_policies_anything);
  FRIEND_TEST(ObjectStoreTest, caching_policies_no_caching);
  FRIEND_TEST(ObjectStoreTest, caching_policies_blob);
  FRIEND_TEST(ObjectStoreTest, caching_policies_trees);
  FRIEND_TEST(ObjectStoreTest, caching_policies_blob_aux_data);
  FRIEND_TEST(ObjectStoreTest, caching_policies_trees_and_blob_aux_data);
  // Forbidden constructor. Use create().
  ObjectStore(
      std::shared_ptr<BackingStore> backingStore,
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<TreeCache> treeCache,
      EdenStatsPtr stats,
      std::shared_ptr<ProcessInfoCache> processInfoCache,
      std::shared_ptr<StructuredLogger> structuredLogger,
      std::shared_ptr<const EdenConfig> edenConfig,
      bool windowsSymlinksEnabled,
      CaseSensitivity caseSensitive);
  // Forbidden copy constructor and assignment operator
  ObjectStore(ObjectStore const&) = delete;
  ObjectStore& operator=(ObjectStore const&) = delete;

  Hash32 computeBlake3(const Blob& blob) const;

  /**
   * Check if the object should be cached in the LocalStore. If
   * localStoreCachingPolicy_ is set to NoCaching, this will always return
   * false.
   */
  bool shouldCacheOnDisk(BackingStore::LocalStoreCachingPolicy object) const;

  /*
   * This method should only be used for testing purposes.
   */
  void setLocalStoreCachingPolicy(
      BackingStore::LocalStoreCachingPolicy policy) {
    localStoreCachingPolicy_ = policy;
  }

  folly::SemiFuture<BackingStore::GetTreeResult> getTreeImpl(
      const ObjectId& id,
      const ObjectFetchContextPtr& context,
      folly::stop_watch<std::chrono::milliseconds> watch) const;

  void maybeCacheTreeAndAuxInLocalStore(
      const ObjectId& id,
      const BackingStore::GetTreeResult& treeResult) const;
  void maybeCacheTreeAuxInMemCache(
      const ObjectId& id,
      const BackingStore::GetTreeResult& treeResult) const;

  folly::SemiFuture<BackingStore::GetTreeAuxResult> getTreeAuxDataImpl(
      const ObjectId& id,
      const ObjectFetchContextPtr& context,
      folly::stop_watch<std::chrono::milliseconds> watch) const;

  folly::SemiFuture<BackingStore::GetBlobResult> getBlobImpl(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const;

  folly::SemiFuture<BackingStore::GetBlobAuxResult> getBlobAuxDataImpl(
      const ObjectId& id,
      const ObjectFetchContextPtr& context,
      folly::stop_watch<std::chrono::milliseconds> watch) const;

  ImmediateFuture<BackingStore::GetGlobFilesResult> getGlobFilesImpl(
      const RootId& id,
      const std::vector<std::string>& globs,
      const std::vector<std::string>& prefixes,
      const ObjectFetchContextPtr& context) const;

  /**
   * During status and checkout, it's common to look up the SHA-1 for a given
   * blob ID. To avoid needing to hit RocksDB, keep a bounded in-memory cache of
   * the sizes and SHA-1s of blobs we've seen. Each node is somewhere around 50
   * bytes (20+28 + LRU overhead) and we store auxDataCacheSize entries (as
   * defined in EdenConfig.h), which EvictingCacheMap divides in two for some
   * reason. At the time of this comment, EvictingCacheMap does not store its
   * nodes densely, so there may also be some jemalloc tracking overhead and
   * some internal fragmentation depending on whether the node fits cleanly
   * into one of jemalloc's size classes.
   *
   * TODO: It never makes sense to rlock an LRU cache, since cache hits mutate
   * the data structure. Thus, should we use a more appropriate type of lock?
   */
  mutable folly::Synchronized<folly::EvictingCacheMap<ObjectId, BlobAuxData>>
      blobAuxDataCache_;

  mutable folly::Synchronized<folly::EvictingCacheMap<ObjectId, TreeAuxData>>
      treeAuxDataCache_;

  /**
   * During glob, we need to read a lot of trees, but we avoid loading inodes,
   * so this means we go to RocksDB for each tree read. To avoid needing to hit
   * RocksDB, keep a bounded in-memory cache of the trees we've seen.
   * This cache will also be read from the first time we load a tree inode.
   * This cache is shared across all object stores, and has a fixed memory
   * size. (The one size limit violation is if there are very large trees,
   * the cache is allowed to retain a small fixed number of these in cache, and
   * violate the fixed size. This generally, should be rare as no trees should
   * approach the size limit of the cache.)
   *
   * TODO: treeCache_ is a shared across all object stores. We should explore
   * having two tree caches (case sensitive and case insensitive) and give the
   * ObjectStore the corresponding tree cache at creation time.
   */
  const std::shared_ptr<TreeCache> treeCache_;

  /*
   * The BackingStore.
   *
   * Multiple ObjectStores may share the same BackingStore.
   */
  std::shared_ptr<BackingStore> backingStore_;

  /*
   * The on-disk cache which is used with most of the BackingStore
   */
  std::shared_ptr<LocalStore> localStore_;

  /*
   * BackingStore can turn off on-disk cache by setting this to
   * LocalStoreCachingPolicy::NoCaching
   *
   * TODO: This might be better suited to either live in the BackingStore, or
   * be determined by querying the BackingStore, as opposed to being a
   * constructor argument, since this is strongly tied to that class.
   */
  BackingStore::LocalStoreCachingPolicy localStoreCachingPolicy_;

  EdenStatsPtr const stats_;

  /* number of fetches for each process collected
   * from the beginning of the eden daemon progress */
  std::unique_ptr<PidFetchCounts> pidFetchCounts_;

  /* process name cache and structured logger used for
   * sending fetch heavy events, set to nullptr if not
   * initialized by create()
   */
  std::shared_ptr<ProcessInfoCache> processInfoCache_;
  std::shared_ptr<StructuredLogger> structuredLogger_;
  std::shared_ptr<const EdenConfig> edenConfig_;

  // Is this ObjectStore case sensitive? This only matters for methods returning
  // Tree.
  CaseSensitivity caseSensitive_;

  // Whether symlinks are enabled on Windows or not
  bool windowsSymlinksEnabled_;
};

} // namespace facebook::eden
