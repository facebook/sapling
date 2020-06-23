/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Executor.h>
#include <folly/Synchronized.h>
#include <folly/container/EvictingCacheMap.h>
#include <memory>
#include <unordered_map>

#include <folly/logging/xlog.h>
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook {
namespace eden {

class BackingStore;
class Blob;
class LocalStore;
class Tree;

struct PidFetchCounts {
  folly::Synchronized<std::unordered_map<pid_t, uint64_t>> map_;

  void recordProcessFetch(pid_t pid) {
    auto map_lock = map_.wlock();
    (*map_lock)[pid]++;
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
                    public std::enable_shared_from_this<ObjectStore> {
 public:
  static std::shared_ptr<ObjectStore> create(
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<BackingStore> backingStore,
      std::shared_ptr<EdenStats> stats,
      folly::Executor::KeepAlive<folly::Executor> executor);
  ~ObjectStore() override;

  /**
   * Get a Tree by ID.
   *
   * This returns a Future object that will produce the Tree when it is ready.
   * It may result in a std::domain_error if the specified tree ID does not
   * exist, or possibly other exceptions on error.
   */
  folly::Future<std::shared_ptr<const Tree>> getTree(
      const Hash& id,
      ObjectFetchContext& context) const override;

  /**
   * Get a commit's root Tree.
   *
   * This returns a Future object that will produce the root Tree when it is
   * ready.  It may result in a std::domain_error if the specified commit ID
   * does not exist, or possibly other exceptions on error.
   */
  folly::Future<std::shared_ptr<const Tree>> getTreeForCommit(
      const Hash& commitID,
      ObjectFetchContext& context) const override;

  folly::Future<std::shared_ptr<const Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID,
      ObjectFetchContext& context) const override;

  folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids,
      ObjectFetchContext& context) const override;

  /**
   * Get a Blob by ID.
   *
   * This returns a Future object that will produce the Blob when it is ready.
   * It may result in a std::domain_error if the specified blob ID does not
   * exist, or possibly other exceptions on error.
   */
  folly::Future<std::shared_ptr<const Blob>> getBlob(
      const Hash& id,
      ObjectFetchContext& context,
      ImportPriority priority = ImportPriority::kNormal()) const override;

  /**
   * Returns the size of the contents of the blob with the given ID.
   */
  folly::Future<uint64_t> getBlobSize(
      const Hash& id,
      ObjectFetchContext& context) const;

  /**
   * Returns the SHA-1 hash of the contents of the blob with the given ID.
   */
  folly::Future<Hash> getBlobSha1(const Hash& id, ObjectFetchContext& context)
      const;

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

  folly::Synchronized<std::unordered_map<pid_t, uint64_t>>& getPidFetches() {
    return pidFetchCounts_->map_;
  }

 private:
  // Forbidden constructor. Use create().
  ObjectStore(
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<BackingStore> backingStore,
      std::shared_ptr<EdenStats> stats,
      folly::Executor::KeepAlive<folly::Executor> executor);
  // Forbidden copy constructor and assignment operator
  ObjectStore(ObjectStore const&) = delete;
  ObjectStore& operator=(ObjectStore const&) = delete;

  /**
   * Get metadata about a Blob.
   *
   * This returns a Future object that will produce the BlobMetadata when it is
   * ready.  It may result in a std::domain_error if the specified blob does
   * not exist, or possibly other exceptions on error.
   */
  folly::Future<BlobMetadata> getBlobMetadata(
      const Hash& id,
      ObjectFetchContext& context) const;

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
  mutable folly::Synchronized<folly::EvictingCacheMap<Hash, BlobMetadata>>
      metadataCache_;

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

  folly::Executor::KeepAlive<folly::Executor> executor_;

  /* number of fetches for each process collected
   * from the beginning of the eden daemon progress */
  std::unique_ptr<PidFetchCounts> pidFetchCounts_;

  void updateBlobStats(bool local, bool backing) const;
  void updateBlobMetadataStats(bool memory, bool local, bool backing) const;
};

} // namespace eden
} // namespace facebook
