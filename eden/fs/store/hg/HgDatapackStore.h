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

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/BlobFwd.h"
#include "eden/fs/model/BlobMetadataFwd.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/store/hg/HgBackingStoreOptions.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
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
  using ImportRequestsList = std::vector<std::shared_ptr<HgImportRequest>>;
  using SaplingNativeOptions = sapling::SaplingNativeBackingStoreOptions;

  /**
   * FaultInjector must be valid for the lifetime of the HgDatapackStore.
   * Currently, FaultInjector is one of the last things destructed when Eden
   * shutsdown. Likely we should use shared pointers instead of raw pointers
   * for FaultInjector though. TODO: T171327256.
   */
  HgDatapackStore(
      sapling::SaplingNativeBackingStore* store,
      HgBackingStoreOptions* runtimeOptions,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<StructuredLogger> logger,
      FaultInjector* FOLLY_NONNULL faultInjector)
      : store_{store},
        runtimeOptions_{runtimeOptions},
        config_{std::move(config)},
        logger_{std::move(logger)},
        faultInjector_{*faultInjector} {}

  std::string_view getRepoName() const {
    return store_->getRepoName();
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
   * Imports the tree identified by the given hash from the remote store.
   * Returns nullptr if not found.
   */
  folly::Try<TreePtr> getTreeRemote(
      const RelativePath& path,
      const Hash20& manifestId,
      const ObjectId& edenTreeId,
      const ObjectFetchContextPtr& context);

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
  folly::Try<BlobPtr> getBlob(
      const HgProxyHash& hgInfo,
      sapling::FetchMode fetchMode);

  /**
   * Imports the blob identified by the given hash from the local store.
   * Returns nullptr if not found.
   */
  folly::Try<BlobPtr> getBlobLocal(const HgProxyHash& hgInfo) {
    return getBlob(hgInfo, sapling::FetchMode::LocalOnly);
  }

  /**
   * Imports the blob identified by the given hash from the remote store.
   * Returns nullptr if not found.
   */
  folly::Try<BlobPtr> getBlobRemote(const HgProxyHash& hgInfo) {
    return getBlob(hgInfo, sapling::FetchMode::RemoteOnly);
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

  // Raw pointer to the `std::unique_ptr<sapling::SaplingNativeBackingStore>`
  // owned by the same `HgQueuedBackingStore` that also has a `std::unique_ptr`
  // to this class. Holding this raw pointer is safe because this class's
  // lifetime is controlled by the same class (`HgQueuedBackingStore`) that
  // controls the lifetime of the underlying
  // `sapling::SaplingNativeBackingStore` here
  sapling::SaplingNativeBackingStore* store_;

  // Raw pointer to the `std::unique_ptr<HgBackingStoreOptions>` owned
  // by the same `HgQueuedBackingStore` that also has a `std::unique_ptr` to
  // this class. Holding this raw pointer is safe because this class's lifetime
  // is controlled by the same class (`HgQueuedBackingStore`) that controls the
  // lifetime of the underlying `HgBackingStoreOptions` here
  HgBackingStoreOptions* runtimeOptions_;
  std::shared_ptr<ReloadableConfig> config_;
  std::shared_ptr<StructuredLogger> logger_;
  FaultInjector& faultInjector_;

  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveBatchedBlobMetaWatches_;
};

} // namespace facebook::eden
