/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/futures/Promise.h>

#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/scm/lib/backingstore/c_api/HgNativeBackingStore.h"

namespace facebook::eden {

class Blob;
class BlobMetadata;
class Hash20;
class HgProxyHash;
class HgImportRequest;
class ObjectId;
class ReloadableConfig;
class Tree;

class HgDatapackStore {
 public:
  HgDatapackStore(
      AbsolutePathPiece repository,
      bool useEdenApi,
      bool useAuxData,
      bool allowRetries,
      std::shared_ptr<ReloadableConfig> config)
      : store_{repository.stringPiece(), useEdenApi, useAuxData, allowRetries},
        config_{std::move(config)} {}

  /**
   * Imports the blob identified by the given hash from the local store.
   * Returns nullptr if not found.
   */
  std::unique_ptr<Blob> getBlobLocal(
      const ObjectId& id,
      const HgProxyHash& hgInfo);

  /**
   * Imports the tree identified by the given hash from the local store.
   * Returns nullptr if not found.
   */
  std::unique_ptr<Tree> getTreeLocal(
      const ObjectId& edenTreeId,
      const HgProxyHash& proxyHash);

  /**
   * Import multiple blobs at once. The vector parameters have to be the same
   * length. Promises passed in will be resolved if a blob is successfully
   * imported. Otherwise the promise will be left untouched.
   */
  void getBlobBatch(
      const std::vector<std::shared_ptr<HgImportRequest>>& requests);

  void getTreeBatch(
      const std::vector<std::shared_ptr<HgImportRequest>>& requests);

  std::unique_ptr<Tree> getTree(
      const RelativePath& path,
      const Hash20& manifestId,
      const ObjectId& edenTreeId);

  /**
   * Reads blob metadata from hg cache.
   */
  std::unique_ptr<BlobMetadata> getLocalBlobMetadata(const Hash20& id);

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

 private:
  HgNativeBackingStore store_;
  std::shared_ptr<ReloadableConfig> config_;

  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedTreeWatches_;
};

} // namespace facebook::eden
