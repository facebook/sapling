/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/futures/Promise.h>

#include "eden/fs/model/BlobFwd.h"
#include "eden/fs/model/BlobMetadataFwd.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/scm/lib/backingstore/c_api/SaplingNativeBackingStore.h"

namespace facebook::eden {

class Hash20;
class HgProxyHash;
class HgImportRequest;
class ObjectId;
class ReloadableConfig;
class StructuredLogger;

class HgDatapackStore {
 public:
  using Options = sapling::BackingStoreOptions;

  HgDatapackStore(
      AbsolutePathPiece repository,
      const Options& options,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<StructuredLogger> logger,
      std::string repoName)
      : store_{repository.view(), options},
        config_{std::move(config)},
        logger_{std::move(logger)},
        repoName_{std::move(repoName)} {}

  void getTreeBatch(
      const std::vector<std::shared_ptr<HgImportRequest>>& requests);

  TreePtr getTree(
      const RelativePath& path,
      const Hash20& manifestId,
      const ObjectId& edenTreeId);

  /**
   * Imports the tree identified by the given hash from the local store.
   * Returns nullptr if not found.
   */
  TreePtr getTreeLocal(
      const ObjectId& edenTreeId,
      const HgProxyHash& proxyHash);

  /**
   * Import multiple blobs at once. The vector parameters have to be the same
   * length. Promises passed in will be resolved if a blob is successfully
   * imported. Otherwise the promise will be left untouched.
   */
  void getBlobBatch(
      const std::vector<std::shared_ptr<HgImportRequest>>& requests);

  /**
   * Imports the blob identified by the given hash from the local store.
   * Returns nullptr if not found.
   */
  BlobPtr getBlobLocal(const HgProxyHash& hgInfo);

  /**
   * Reads blob metadata from hg cache.
   */
  BlobMetadataPtr getLocalBlobMetadata(const HgProxyHash& id);

  /**
   * Fetch multiple aux data at once.
   *
   * This function returns when all the aux data have been fetched.
   */
  void getBlobMetadataBatch(
      const std::vector<std::shared_ptr<HgImportRequest>>& requests);

  /**
   * Flush any pending writes to disk.
   *
   * As a side effect, this also reloads the current state of Mercurial's
   * cache, picking up any writes done by Mercurial.
   */
  void flush();

  /**
   * Get the metrics tracking the number of live batched blobs.
   */
  RequestMetricsScope::LockedRequestWatchList& getLiveBatchedBlobWatches()
      const {
    return liveBatchedBlobWatches_;
  }

  /**
   * Get the metrics tracking the number of live batched trees.
   */
  RequestMetricsScope::LockedRequestWatchList& getLiveBatchedTreeWatches()
      const {
    return liveBatchedTreeWatches_;
  }

  /**
   * Get the metrics tracking the number of live batched aux data.
   */
  RequestMetricsScope::LockedRequestWatchList& getLiveBatchedBlobMetaWatches()
      const {
    return liveBatchedBlobMetaWatches_;
  }

 private:
  sapling::SaplingNativeBackingStore store_;
  std::shared_ptr<ReloadableConfig> config_;
  std::shared_ptr<StructuredLogger> logger_;
  std::string repoName_;

  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveBatchedBlobMetaWatches_;
};

} // namespace facebook::eden
