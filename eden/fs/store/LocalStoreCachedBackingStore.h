/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/BackingStore.h"
#include "eden/fs/utils/RefPtr.h"

namespace facebook::eden {

class BackingStore;
class LocalStore;
class EdenStats;

using EdenStatsPtr = RefPtr<EdenStats>;

/**
 * Implementation of a BackingStore that caches the returned data from another
 * BackingStore onto the LocalStore.
 *
 * Reads will first attempt to read from the LocalStore, and will only read
 * from the underlying BackingStore if the data wasn't found in the LocalStore.
 *
 * This should be used for BackingStores that either do not have local caching
 * builtin, or when reading from this cache is significantly slower than
 * reading from the LocalStore.
 */
class LocalStoreCachedBackingStore
    : public BackingStore,
      public std::enable_shared_from_this<LocalStoreCachedBackingStore> {
 public:
  /**
   * Policy describing the kind of data cached in the LocalStore.
   */
  enum class CachingPolicy {
    NoCaching = 0,
    Trees = 1 << 0,
    Blobs = 1 << 1,
    BlobMetadata = 1 << 2,
    TreesAndBlobMetadata = Trees | BlobMetadata,
    Everything = Trees | Blobs | BlobMetadata,
  };

  LocalStoreCachedBackingStore(
      std::shared_ptr<BackingStore> backingStore,
      std::shared_ptr<LocalStore> localStore,
      EdenStatsPtr stats,
      CachingPolicy cachingPolicy);
  ~LocalStoreCachedBackingStore() override;

  ObjectComparison compareObjectsById(const ObjectId& one, const ObjectId& two)
      override;

  ImmediateFuture<GetRootTreeResult> getRootTree(
      const RootId& rootId,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<std::shared_ptr<TreeEntry>> getTreeEntryForObjectId(
      const ObjectId& objectId,
      TreeEntryType treeEntryType,
      const ObjectFetchContextPtr& context) override;
  folly::SemiFuture<GetTreeResult> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  folly::SemiFuture<GetBlobResult> getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  folly::SemiFuture<GetBlobMetaResult> getBlobMetadata(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;

  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> prefetchBlobs(
      ObjectIdRange ids,
      const ObjectFetchContextPtr& context) override;

  void periodicManagementTask() override;

  void startRecordingFetch() override;
  std::unordered_set<std::string> stopRecordingFetch() override;

  ImmediateFuture<folly::Unit> importManifestForRoot(
      const RootId& rootId,
      const Hash20& manifest,
      const ObjectFetchContextPtr& context) override;

  RootId parseRootId(folly::StringPiece rootId) override;
  std::string renderRootId(const RootId& rootId) override;
  ObjectId parseObjectId(folly::StringPiece objectId) override;
  std::string renderObjectId(const ObjectId& objectId) override;

  std::optional<folly::StringPiece> getRepoName() override;

  /**
   * Get the underlying BackingStore. This should only be used for operations
   * that need to be made directly on the BackingStore, like getting a TraceBus
   */
  const std::shared_ptr<BackingStore>& getBackingStore() {
    return backingStore_;
  }

  // TODO(T119221752): Implement for all BackingStore subclasses
  int64_t dropAllPendingRequestsFromQueue() override {
    XLOG(
        WARN,
        "dropAllPendingRequestsFromQueue() is not implemented for LocalStoreCachedBackingStore");
    return 0;
  }

 private:
  /**
   * Test if the object should be cached in the LocalStore.
   */
  bool shouldCache(CachingPolicy object) const;

  std::shared_ptr<BackingStore> backingStore_;
  std::shared_ptr<LocalStore> localStore_;
  EdenStatsPtr stats_;
  CachingPolicy cachingPolicy_;
};

} // namespace facebook::eden
