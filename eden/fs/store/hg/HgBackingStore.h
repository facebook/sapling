/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <optional>

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

/* forward declare support classes from mercurial */
class ConstantStringRef;
class DatapackStore;
class UnionDatapackStore;

namespace facebook {
namespace eden {

class HgImporter;
struct ImporterOptions;
class EdenStats;
class LocalStore;
class UnboundedQueueExecutor;
class ReloadableConfig;
class ServiceAddress;
class HgProxyHash;

/**
 * A BackingStore implementation that loads data out of a mercurial repository.
 */
class HgBackingStore : public BackingStore {
 public:
  /**
   * Create a new HgBackingStore.
   */
  HgBackingStore(
      AbsolutePathPiece repository,
      std::shared_ptr<LocalStore> localStore,
      UnboundedQueueExecutor* serverThreadPool,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<EdenStats>);

  /**
   * Create an HgBackingStore suitable for use in unit tests. It uses an inline
   * executor to process loaded objects rather than the thread pools used in
   * production Eden.
   */
  HgBackingStore(
      AbsolutePathPiece repository,
      HgImporter* importer,
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<EdenStats>);

  ~HgBackingStore() override;

  folly::SemiFuture<std::unique_ptr<Tree>> getTree(
      const Hash& id,
      ObjectFetchContext& context,
      ImportPriority priority = ImportPriority::kNormal()) override;
  folly::SemiFuture<std::unique_ptr<Blob>> getBlob(
      const Hash& id,
      ObjectFetchContext& context,
      ImportPriority priority = ImportPriority::kNormal()) override;
  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) override;
  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids) override;

  void periodicManagementTask() override;

  /**
   * Import the manifest for the specified revision using mercurial
   * treemanifest data.
   */
  folly::Future<std::unique_ptr<Tree>> importTreeManifest(const Hash& commitId);

  /**
   * Objects that can be imported from Hg
   */
  enum HgImportObject { BLOB, TREE, PREFETCH };

  constexpr static std::array<HgImportObject, 3> hgImportObjects{
      HgImportObject::BLOB,
      HgImportObject::TREE,
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

  /**
   * Retrieve a blob from hgcache. This function may return `nullptr` when it
   * couldn't fetch the blob.
   */
  std::unique_ptr<Blob> getBlobFromHgCache(
      const Hash& id,
      const HgProxyHash& hgInfo);
  folly::SemiFuture<std::unique_ptr<Blob>> fetchBlobFromHgImporter(
      HgProxyHash hgInfo);

  HgDatapackStore& getDatapackStore() {
    return datapackStore_;
  }

 private:
  // Forbidden copy constructor and assignment operator
  HgBackingStore(HgBackingStore const&) = delete;
  HgBackingStore& operator=(HgBackingStore const&) = delete;

  /**
   * Initialize the unionStore_ needed for treemanifest import support.
   */
  void initializeTreeManifestImport(
      const ImporterOptions& options,
      AbsolutePathPiece repoPath);

  folly::Future<std::unique_ptr<Tree>> getTreeForCommitImpl(Hash commitID);

  folly::Future<std::unique_ptr<Tree>> getTreeForRootTreeImpl(
      const Hash& commitID,
      const Hash& rootTreeHash);

  // Import the Tree from Hg and cache it in the LocalStore before returning it.
  folly::SemiFuture<std::unique_ptr<Tree>> importTreeForCommit(Hash commitID);

  void initializeDatapackImport(AbsolutePathPiece repository);
  folly::Future<std::unique_ptr<Tree>> importTreeImpl(
      const Hash& manifestNode,
      const Hash& edenTreeID,
      RelativePathPiece path,
      const std::optional<Hash>& commitHash);
  folly::Future<std::unique_ptr<Tree>> fetchTreeFromHgCacheOrImporter(
      Hash manifestNode,
      Hash edenTreeID,
      RelativePath path,
      const std::optional<Hash>& commitId);
  folly::Future<std::unique_ptr<Tree>> fetchTreeFromImporter(
      Hash manifestNode,
      Hash edenTreeID,
      RelativePath path,
      std::optional<Hash> commitId,
      std::shared_ptr<LocalStore::WriteBatch> writeBatch);
  std::unique_ptr<Tree> processTree(
      ConstantStringRef& content,
      const Hash& manifestNode,
      const Hash& edenTreeID,
      RelativePathPiece path,
      const std::optional<Hash>& commitHash,
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

  // These DatapackStore objects are never referenced once UnionDatapackStore
  // is allocated. They are here solely so their lifetime persists while the
  // UnionDatapackStore is alive.
  std::vector<std::unique_ptr<DatapackStore>> dataPackStores_;
  std::unique_ptr<folly::Synchronized<UnionDatapackStore>> unionStore_;

  std::string repoName_;
  HgDatapackStore datapackStore_;

  // Track metrics for imports currently fetching data from hg
  mutable RequestMetricsScope::LockedRequestWatchList liveImportBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveImportTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveImportPrefetchWatches_;
};
} // namespace eden
} // namespace facebook
