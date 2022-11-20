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
#include <folly/String.h>
#include <folly/Synchronized.h>

#include "eden/fs/eden-config.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgDatapackStore.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class HgImporter;
struct ImporterOptions;
class EdenStats;
class LocalStore;
class UnboundedQueueExecutor;
class ReloadableConfig;
class HgProxyHash;
class StructuredLogger;

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
      std::shared_ptr<EdenStats> edenStats,
      std::shared_ptr<StructuredLogger> logger);

  /**
   * Create an HgBackingStore suitable for use in unit tests. It uses an inline
   * executor to process loaded objects rather than the thread pools used in
   * production Eden.
   */
  HgBackingStore(
      AbsolutePathPiece repository,
      HgImporter* importer,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<EdenStats>);

  ~HgBackingStore();

  ImmediateFuture<std::unique_ptr<Tree>> getRootTree(const RootId& rootId);
  folly::SemiFuture<std::unique_ptr<Tree>> getTree(
      const std::shared_ptr<HgImportRequest>& request);

  void periodicManagementTask();

  /**
   * Import the root manifest for the specied revision using mercurial
   * treemanifest data.  This is called when the root manifest is provided
   * to EdenFS directly by the hg client.
   */
  folly::Future<folly::Unit> importTreeManifestForRoot(
      const RootId& rootId,
      const Hash20& manifestId);

  /**
   * Import the manifest for the specified revision using mercurial
   * treemanifest data.
   */
  folly::Future<std::unique_ptr<Tree>> importTreeManifest(
      const ObjectId& commitId);

  /**
   * Objects that can be imported from Hg
   */
  enum HgImportObject { BLOB, TREE, BATCHED_BLOB, BATCHED_TREE, PREFETCH };

  constexpr static std::array<HgImportObject, 5> hgImportObjects{
      HgImportObject::BLOB,
      HgImportObject::TREE,
      HgImportObject::BATCHED_BLOB,
      HgImportObject::BATCHED_TREE,
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

  folly::SemiFuture<std::unique_ptr<Blob>> fetchBlobFromHgImporter(
      HgProxyHash hgInfo);

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

  folly::Future<std::unique_ptr<Tree>> importTreeManifestImpl(
      Hash20 manifestNode);

  void initializeDatapackImport(AbsolutePathPiece repository);
  folly::Future<std::unique_ptr<Tree>> importTreeImpl(
      const Hash20& manifestNode,
      const ObjectId& edenTreeID,
      RelativePathPiece path);
  folly::Future<std::unique_ptr<Tree>> fetchTreeFromHgCacheOrImporter(
      Hash20 manifestNode,
      ObjectId edenTreeID,
      RelativePath path);
  folly::Future<std::unique_ptr<Tree>> fetchTreeFromImporter(
      Hash20 manifestNode,
      ObjectId edenTreeID,
      RelativePath path,
      std::shared_ptr<LocalStore::WriteBatch> writeBatch);
  std::unique_ptr<Tree> processTree(
      std::unique_ptr<folly::IOBuf> content,
      const Hash20& manifestNode,
      const ObjectId& edenTreeID,
      RelativePathPiece path,
      LocalStore::WriteBatch* writeBatch);

  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<EdenStats> stats_;
  // A set of threads owning HgImporter instances
  std::unique_ptr<folly::Executor> importThreadPool_;
  std::shared_ptr<ReloadableConfig> config_;
  // The main server thread pool; we push the Futures back into
  // this pool to run their completion code to avoid clogging
  // the importer pool. Queuing in this pool can never block (which would risk
  // deadlock) or throw an exception when full (which would incorrectly fail the
  // load).
  folly::Executor* serverThreadPool_;

  std::string repoName_;
  HgDatapackStore datapackStore_;

  std::shared_ptr<StructuredLogger> logger_;

  // Track metrics for imports currently fetching data from hg
  mutable RequestMetricsScope::LockedRequestWatchList liveImportBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveImportTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveImportPrefetchWatches_;
};

} // namespace facebook::eden
