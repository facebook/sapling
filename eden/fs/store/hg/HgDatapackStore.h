/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/futures/Promise.h>
#include <optional>
#include <string_view>

#include "eden/fs/model/BlobFwd.h"
#include "eden/fs/model/BlobMetadataFwd.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/scm/lib/backingstore/include/SaplingNativeBackingStore.h"

namespace facebook::eden {

class Hash20;
class HgProxyHash;
class HgImportRequest;
class ObjectId;
class ReloadableConfig;
class StructuredLogger;
class FaultInjector;
template <typename T>
class RefPtr;
class ObjectFetchContext;
using ObjectFetchContextPtr = RefPtr<ObjectFetchContext>;

class HgDatapackStore {
 public:
  using Options = sapling::BackingStoreOptions;
  using ImportRequestsList = std::vector<std::shared_ptr<HgImportRequest>>;

  /**
   * FaultInjector must be valid for the lifetime of the HgDatapackStore.
   * Currently, FaultInjector is one of the last things destructed when Eden
   * shutsdown. Likely we should use shared pointers instead of raw pointers
   * for FaultInjector though. TODO: T171327256.
   */
  HgDatapackStore(
      AbsolutePathPiece repository,
      const Options& options,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<StructuredLogger> logger,
      FaultInjector* FOLLY_NONNULL faultInjector)
      : store_{repository.view(), options},
        config_{std::move(config)},
        logger_{std::move(logger)},
        faultInjector_{*faultInjector} {}

  std::string_view getRepoName() const {
    return store_.getRepoName();
  }

  std::optional<Hash20> getManifestNode(const ObjectId& commitId);

  void getTreeBatch(const ImportRequestsList& requests);

  folly::Try<TreePtr> getTree(
      const RelativePath& path,
      const Hash20& manifestId,
      const ObjectId& edenTreeId,
      const ObjectFetchContextPtr& context);

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
  void getBlobBatch(const ImportRequestsList& requests);

  /**
   * Imports the blob identified by the given hash from the backing store.
   * If localOnly is set to true, only fetch the blob from local (memory or
   * disk) store.
   *
   * Returns nullptr if not found.
   */
  folly::Try<BlobPtr> getBlob(const HgProxyHash& hgInfo, bool localOnly);

  /**
   * Imports the blob identified by the given hash from the local store.
   * Returns nullptr if not found.
   */
  folly::Try<BlobPtr> getBlobLocal(const HgProxyHash& hgInfo) {
    return getBlob(hgInfo, /*localOnly=*/true);
  }

  /**
   * Reads blob metadata from hg cache.
   */
  folly::Try<BlobMetadataPtr> getLocalBlobMetadata(const HgProxyHash& id);

  /**
   * Fetch multiple aux data at once.
   *
   * This function returns when all the aux data have been fetched.
   */
  void getBlobMetadataBatch(const ImportRequestsList& requests);

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
  using ImportRequestsMap = std::
      map<sapling::NodeId, std::pair<ImportRequestsList, RequestMetricsScope>>;

  template <typename T>
  std::pair<HgDatapackStore::ImportRequestsMap, std::vector<sapling::NodeId>>
  prepareRequests(
      const ImportRequestsList& importRequests,
      const std::string& requestType);

  sapling::SaplingNativeBackingStore store_;
  std::shared_ptr<ReloadableConfig> config_;
  std::shared_ptr<StructuredLogger> logger_;
  FaultInjector& faultInjector_;

  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveBatchedBlobMetaWatches_;
};

} // namespace facebook::eden
