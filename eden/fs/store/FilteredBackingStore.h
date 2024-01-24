/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/filter/Filter.h"
#include "eden/fs/store/filter/FilteredObjectId.h"
#include "eden/fs/utils/PathMap.h"
#include "eden/fs/utils/RefPtr.h"

namespace facebook::eden {

class BackingStore;

/**
 * Implementation of a BackingStore that allows filtering sets odf paths from
 * the checkout.
 *
 * The FilteredBackingStore filters paths at the tree level, so much of the
 * blob implementation is the same. Filtering is achieved by never creating
 * FilteredObjectIds for paths contained in the filter list.
 */
class FilteredBackingStore
    : public BackingStore,
      public std::enable_shared_from_this<FilteredBackingStore> {
 public:
  FilteredBackingStore(
      std::shared_ptr<BackingStore> backingStore,
      std::unique_ptr<Filter> filter);

  ~FilteredBackingStore() override;

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
   * Encodes an underlying RootId in the RootId format used by
   * FilteredBackingStore. This format is as follows:
   *
   * <originalRootIdLength><originalRootId><filterId>
   *
   * Where originalRootIdLength is a varint representing the length of the
   * original RootId. This is used so we can properly parse out the filterId
   * from the RootID at a later point in time.
   */
  static std::string createFilteredRootId(
      std::string_view originalRootId,
      std::string_view filterId);

  /**
   * Similar to createFilteredRootId, but uses the null filter ID instead of a
   * user provided filter ID.
   */
  static std::string createNullFilteredRootId(std::string_view originalRootId) {
    return createFilteredRootId(originalRootId, kNullFilterId);
  }

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
        "dropAllPendingRequestsFromQueue() is not implemented for FilteredBackingStore");
    return 0;
  }

 private:
  std::shared_ptr<BackingStore> backingStore_;

  // Allows FilteredBackingStore creator to specify how they want to filter
  // paths. This returns true if the given path is filtered in the given
  // filterId
  std::unique_ptr<Filter> filter_;

  /*
   * Does the actual filtering logic for tree and root-tree objects.
   */
  ImmediateFuture<std::unique_ptr<PathMap<TreeEntry>>> filterImpl(
      const TreePtr unfilteredTree,
      RelativePathPiece treePath,
      folly::StringPiece filterId,
      FilteredObjectIdType treeType);

  /*
   * Determine whether a path is affected by a filter change from One -> Two or
   * vice versa.
   */
  ImmediateFuture<ObjectComparison> pathAffectedByFilterChange(
      RelativePathPiece pathOne,
      RelativePathPiece pathTwo,
      folly::StringPiece filterIdOne,
      folly::StringPiece filterIdTwo);
};

} // namespace facebook::eden
