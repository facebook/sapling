/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/BackingStore.h"

namespace facebook::eden {

class BackingStore;
class LocalStore;
class EdenStats;

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
class LocalStoreCachedBackingStore : public BackingStore {
 public:
  LocalStoreCachedBackingStore(
      std::shared_ptr<BackingStore> backingStore,
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<EdenStats> stats);
  ~LocalStoreCachedBackingStore() override;

  ObjectComparison compareObjectsById(const ObjectId& one, const ObjectId& two)
      override;

  folly::SemiFuture<std::unique_ptr<Tree>> getRootTree(
      const RootId& rootId,
      ObjectFetchContext& context) override;

  folly::SemiFuture<std::unique_ptr<TreeEntry>> getTreeEntryForRootId(
      const RootId& rootId,
      TreeEntryType treeEntryType,
      ObjectFetchContext& context) override;
  folly::SemiFuture<GetTreeRes> getTree(
      const ObjectId& id,
      ObjectFetchContext& context) override;
  folly::SemiFuture<GetBlobRes> getBlob(
      const ObjectId& id,
      ObjectFetchContext& context) override;

  std::unique_ptr<BlobMetadata> getLocalBlobMetadata(
      const ObjectId& id,
      ObjectFetchContext& context) override;

  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> prefetchBlobs(
      ObjectIdRange ids,
      ObjectFetchContext& context) override;

  void periodicManagementTask() override;

  void startRecordingFetch() override;
  std::unordered_set<std::string> stopRecordingFetch() override;

  folly::SemiFuture<folly::Unit> importManifestForRoot(
      const RootId& rootId,
      const Hash20& manifest) override;

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
  std::shared_ptr<BackingStore> backingStore_;
  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<EdenStats> stats_;
};

} // namespace facebook::eden
