/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/Executor.h>
#include <folly/Range.h>
#include <folly/Synchronized.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgBackingStoreOptions.h"
#include "eden/fs/store/hg/HgDatapackStore.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"

namespace facebook::eden {

struct ImporterOptions;
class EdenStats;
class LocalStore;
class UnboundedQueueExecutor;
class ReloadableConfig;
class HgProxyHash;
class StructuredLogger;
class FaultInjector;

using EdenStatsPtr = RefPtr<EdenStats>;

/**
 * An implementation class for HgQueuedBackingStore that loads data out of a
 * mercurial repository.
 */
class HgBackingStore {
 public:
  /**
   * Create a new HgBackingStore.
   */
  HgBackingStore(
      folly::Executor* retryThreadPool,
      std::shared_ptr<LocalStore> localStore,
      HgDatapackStore* datapackStore,
      UnboundedQueueExecutor* serverThreadPool,
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr edenStats,
      std::shared_ptr<StructuredLogger> logger);

  /**
   * Create an HgBackingStore suitable for use in unit tests. It uses an inline
   * executor to process loaded objects rather than the thread pools used in
   * production Eden.
   */
  HgBackingStore(
      folly::Executor* retryThreadPool,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<LocalStore> localStore,
      HgDatapackStore* datapackStore,
      EdenStatsPtr);

  ~HgBackingStore();

  ImmediateFuture<BackingStore::GetRootTreeResult> getRootTree(
      const RootId& rootId,
      const ObjectFetchContextPtr& context);
  folly::SemiFuture<TreePtr> getTree(
      const std::shared_ptr<HgImportRequest>& request);

  /**
   * Import the root manifest for the specied revision using mercurial
   * treemanifest data.  This is called when the root manifest is provided
   * to EdenFS directly by the hg client.
   */
  ImmediateFuture<folly::Unit> importTreeManifestForRoot(
      const RootId& rootId,
      const Hash20& manifestId,
      const ObjectFetchContextPtr& context);

  /**
   * Import the manifest for the specified revision using mercurial
   * treemanifest data.
   */
  folly::Future<TreePtr> importTreeManifest(
      const ObjectId& commitId,
      const ObjectFetchContextPtr& context);

  /**
   * Objects that can be imported from Hg
   */
  enum HgImportObject {
    BLOB,
    TREE,
    BLOBMETA,
    BATCHED_BLOB,
    BATCHED_TREE,
    BATCHED_BLOBMETA,
    PREFETCH
  };

  constexpr static std::array<HgImportObject, 7> hgImportObjects{
      HgImportObject::BLOB,
      HgImportObject::TREE,
      HgImportObject::BLOBMETA,
      HgImportObject::BATCHED_BLOB,
      HgImportObject::BATCHED_TREE,
      HgImportObject::BATCHED_BLOBMETA,
      HgImportObject::PREFETCH};

  static folly::StringPiece stringOfHgImportObject(HgImportObject object);

  /**
   * Gets the watches timing live `object` imports
   *   ex. HgBackingStore::getLiveImportWatches(
   *          RequestMetricsScope::HgImportObject::BLOB,
   *        )
   *    gets the watches timing live blob imports
   */
  RequestMetricsScope::LockedRequestWatchList& getLiveImportWatches(
      HgImportObject object) const;

  // Get blob step functions

  folly::SemiFuture<BlobPtr> fetchBlobFromHgImporter(HgProxyHash hgInfo);

  HgDatapackStore* getDatapackStore() {
    return datapackStore_;
  }

  std::optional<folly::StringPiece> getRepoName() {
    return datapackStore_->getRepoName();
  }

 private:
  // Forbidden copy constructor and assignment operator
  HgBackingStore(HgBackingStore const&) = delete;
  HgBackingStore& operator=(HgBackingStore const&) = delete;

  folly::Future<TreePtr> importTreeManifestImpl(
      Hash20 manifestNode,
      const ObjectFetchContextPtr& context);

  void initializeDatapackImport(AbsolutePathPiece repository);
  folly::Future<TreePtr> importTreeImpl(
      const Hash20& manifestNode,
      const ObjectId& edenTreeID,
      RelativePathPiece path);

  folly::Future<TreePtr> fetchTreeFromImporter(
      Hash20 manifestNode,
      ObjectId edenTreeID,
      RelativePath path,
      std::shared_ptr<LocalStore::WriteBatch> writeBatch);

  std::shared_ptr<LocalStore> localStore_;
  EdenStatsPtr stats_;
  // A set of threads processing Sapling retry requests.
  folly::Executor* retryThreadPool_;
  std::shared_ptr<ReloadableConfig> config_;
  // The main server thread pool; we push the Futures back into
  // this pool to run their completion code to avoid clogging
  // the importer pool. Queuing in this pool can never block (which would risk
  // deadlock) or throw an exception when full (which would incorrectly fail the
  // load).
  folly::Executor* serverThreadPool_;

  std::shared_ptr<StructuredLogger> logger_;

  // Raw pointer to the `std::unique_ptr<HgDatapackStore>` owned by the
  // same `HgQueuedBackingStore` that also has a `std::unique_ptr` to this
  // class. Holding this raw pointer is safe because this class's lifetime is
  // controlled by the same class (`HgQueuedBackingStore`) that controlls the
  // lifetime of the underlying `HgDatapackStore` here
  HgDatapackStore* datapackStore_;

  // Track metrics for imports currently fetching data from hg
  mutable RequestMetricsScope::LockedRequestWatchList liveImportBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveImportTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveImportBlobMetaWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveImportPrefetchWatches_;
};

} // namespace facebook::eden
