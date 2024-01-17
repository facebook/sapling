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

#include "eden/fs/eden-config.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgDatapackStore.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/RefPtr.h"

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
      AbsolutePathPiece repository,
      std::shared_ptr<LocalStore> localStore,
      UnboundedQueueExecutor* serverThreadPool,
      std::shared_ptr<ReloadableConfig> config,
      EdenStatsPtr edenStats,
      std::shared_ptr<StructuredLogger> logger,
      FaultInjector* FOLLY_NONNULL faultInjector);

  /**
   * Create an HgBackingStore suitable for use in unit tests. It uses an inline
   * executor to process loaded objects rather than the thread pools used in
   * production Eden.
   */
  HgBackingStore(
      AbsolutePathPiece repository,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<LocalStore> localStore,
      EdenStatsPtr,
      FaultInjector* FOLLY_NONNULL faultInjector);

  ~HgBackingStore();

  ImmediateFuture<BackingStore::GetRootTreeResult> getRootTree(
      const RootId& rootId,
      const ObjectFetchContextPtr& context);
  folly::SemiFuture<TreePtr> getTree(
      const std::shared_ptr<HgImportRequest>& request);

  void periodicManagementTask();

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

  HgDatapackStore& getDatapackStore() {
    return datapackStore_;
  }

  std::optional<folly::StringPiece> getRepoName() {
    return std::optional<folly::StringPiece>{repoName_};
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
  std::unique_ptr<folly::Executor> retryThreadPool_;
  std::shared_ptr<ReloadableConfig> config_;
  // The main server thread pool; we push the Futures back into
  // this pool to run their completion code to avoid clogging
  // the importer pool. Queuing in this pool can never block (which would risk
  // deadlock) or throw an exception when full (which would incorrectly fail the
  // load).
  folly::Executor* serverThreadPool_;

  std::shared_ptr<StructuredLogger> logger_;

  std::string repoName_;
  HgDatapackStore datapackStore_;

  // Track metrics for imports currently fetching data from hg
  mutable RequestMetricsScope::LockedRequestWatchList liveImportBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveImportTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveImportBlobMetaWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveImportPrefetchWatches_;
};

} // namespace facebook::eden
