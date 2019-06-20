/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/utils/PathFuncs.h"
#ifndef EDEN_WIN_NO_RUST_DATAPACK
#include "scm/hg/lib/revisionstore/RevisionStore.h"
#endif
#include "eden/fs/tracing/EdenStats.h"

#include <folly/Executor.h>
#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <memory>
#include <optional>

/* forward declare support classes from mercurial */
class ConstantStringRef;
class DatapackStore;
class UnionDatapackStore;

namespace facebook {
namespace eden {

class Importer;
struct ImporterOptions;
class LocalStore;
class MononokeHttpBackingStore;
class MononokeThriftBackingStore;
class MononokeCurlBackingStore;
class UnboundedQueueExecutor;
class ReloadableConfig;
class ServiceAddress;

/**
 * A BackingStore implementation that loads data out of a mercurial repository.
 */
class HgBackingStore : public BackingStore {
 public:
  /**
   * Create a new HgBackingStore.
   *
   * The LocalStore object is owned by the EdenServer (which also owns this
   * HgBackingStore object).  It is guaranteed to be valid for the lifetime of
   * the HgBackingStore object.
   */
  HgBackingStore(
      AbsolutePathPiece repository,
      LocalStore* localStore,
      UnboundedQueueExecutor* serverThreadPool,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<EdenStats>);

  /**
   * Create an HgBackingStore suitable for use in unit tests. It uses an inline
   * executor to process loaded objects rather than the thread pools used in
   * production Eden.
   */
  HgBackingStore(
      Importer* importer,
      LocalStore* localStore,
      std::shared_ptr<EdenStats>);

  ~HgBackingStore() override;

  folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  FOLLY_NODISCARD folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids) const override;

  /**
   * Import the manifest for the specified revision using mercurial
   * treemanifest data.
   */
  folly::Future<Hash> importTreeManifest(const Hash& commitId);

 private:
  // Forbidden copy constructor and assignment operator
  HgBackingStore(HgBackingStore const&) = delete;
  HgBackingStore& operator=(HgBackingStore const&) = delete;

  /**
   * Initialize the unionStore_ needed for treemanifest import support.
   *
   * This leaves unionStore_ null if treemanifest import is not supported in
   * this repository.
   */
  void initializeTreeManifestImport(
      const ImporterOptions& options,
      AbsolutePathPiece repoPath);

  /**
   * Create a Mononoke backing store based on config_.
   *
   * Return nullptr if something is wrong (e.g. missing configs).
   */
  std::unique_ptr<BackingStore> initializeMononoke();

  /**
   * Get an instace of Mononoke backing store as specified in config_. This will
   * call `initializeMononoke` if no active Mononoke instance is stored.
   *
   * Return nullptr if Mononoke is disabled.
   */
  std::shared_ptr<BackingStore> getMononoke();

  /**
   * Get an instance of `ServiceAddress` that points to Mononoke API Server
   * based on user's configuration. It could be a pair of host and port or a smc
   * tier name.
   */
  std::unique_ptr<ServiceAddress> getMononokeServiceAddress();

#ifndef EDEN_WIN_NOMONONOKE
  /**
   * Create an instance of MononokeHttpBackingStore with values from config_
   * (Proxygen based Mononoke client)
   *
   * Return null if SSLContext cannot be constructed.
   */
  std::unique_ptr<MononokeHttpBackingStore>
  initializeHttpMononokeBackingStore();

  /**
   * Create an instance of MononokeThriftBackingStore with values from config_
   * (Thrift based Mononoke client)
   *
   * Return nullptr if required config is missing.
   */
  std::unique_ptr<MononokeThriftBackingStore>
  initializeThriftMononokeBackingStore();
#endif

#if defined(EDEN_HAVE_CURL)
  /**
   * Create an instance of MononokeCurlBackingStore with values from config_
   * (Curl based Mononoke client)
   *
   * Return nullptr if required config is missing.
   */
  std::unique_ptr<MononokeCurlBackingStore>
  initializeCurlMononokeBackingStore();
#endif

  folly::Future<std::unique_ptr<Blob>> getBlobFromHgImporter(const Hash& id);

  folly::Future<std::unique_ptr<Tree>> getTreeForCommitImpl(Hash commitID);

  // Import the Tree from Hg and cache it in the LocalStore before returning it.
  folly::Future<std::unique_ptr<Tree>> importTreeForCommit(Hash commitID);

  void initializeDatapackImport(AbsolutePathPiece repository);
  folly::Future<std::unique_ptr<Tree>> importTreeImpl(
      const Hash& manifestNode,
      const Hash& edenTreeID,
      RelativePathPiece path);
  folly::Future<std::unique_ptr<Tree>> fetchTreeFromHgCacheOrImporter(
      Hash manifestNode,
      Hash edenTreeID,
      RelativePath path);
  folly::Future<std::unique_ptr<Tree>> fetchTreeFromImporter(
      Hash manifestNode,
      Hash edenTreeID,
      RelativePath path,
      std::shared_ptr<LocalStore::WriteBatch> writeBatch);
  std::unique_ptr<Tree> processTree(
      ConstantStringRef& content,
      const Hash& manifestNode,
      const Hash& edenTreeID,
      RelativePathPiece path,
      LocalStore::WriteBatch* writeBatch);

  folly::Future<Hash> importManifest(Hash commitId);

  folly::Future<Hash> importFlatManifest(Hash commitId);

  LocalStore* localStore_{nullptr};
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
  bool useDatapackGetBlob_{false};

  std::string repoName_;
  folly::Synchronized<std::shared_ptr<BackingStore>> mononoke_;
#ifndef EDEN_WIN_NO_RUST_DATAPACK
  std::optional<folly::Synchronized<DataPackUnion>> dataPackStore_;
#endif
};
} // namespace eden
} // namespace facebook
