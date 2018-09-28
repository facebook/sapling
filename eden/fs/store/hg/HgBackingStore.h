/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "eden/fs/eden-config.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/mononoke/MononokeBackingStore.h"
#include "eden/fs/utils/PathFuncs.h"

#include <folly/Executor.h>
#include <folly/Range.h>
#include <folly/Synchronized.h>

#if EDEN_HAVE_HG_TREEMANIFEST
/* forward declare support classes from mercurial */
class ConstantStringRef;
class DatapackStore;
class UnionDatapackStore;
#endif // EDEN_HAVE_HG_TREEMANIFEST

namespace facebook {
namespace eden {

class Importer;
class ImporterOptions;
class LocalStore;
class UnboundedQueueExecutor;

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
      folly::Optional<AbsolutePath> clientCertificate = folly::none,
      bool useMononoke = false);

  /**
   * Create an HgBackingStore suitable for use in unit tests. It uses an inline
   * executor to process loaded objects rather than the thread pools used in
   * production Eden.
   */
  HgBackingStore(Importer* importer, LocalStore* localStore);

  ~HgBackingStore() override;

  folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  FOLLY_NODISCARD folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids) const override;

  folly::Future<std::unique_ptr<Blob>> verifyEmptyBlob(const Hash& id) override;

#if EDEN_HAVE_HG_TREEMANIFEST
  /**
   * Import the manifest for the specified revision using mercurial
   * treemanifest data.
   */
  folly::Future<Hash> importTreeManifest(const Hash& commitId);
#endif // EDEN_HAVE_HG_TREEMANIFEST

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
   * Initialize the mononoke_ needed for Mononoke API Server support.
   *
   * This leaves mononoke_ null if mononoke does not support the repository.
   */
  void initializeMononoke(
      const ImporterOptions& options,
      bool useMononoke,
      folly::Optional<AbsolutePath> clientCertificate);

  folly::Future<std::unique_ptr<Tree>> getTreeForCommitImpl(Hash commitID);

  // Import the Tree from Hg and cache it in the LocalStore before returning it.
  folly::Future<std::unique_ptr<Tree>> importTreeForCommit(Hash commitID);

#if EDEN_HAVE_HG_TREEMANIFEST
  folly::Future<std::unique_ptr<Tree>> importTreeImpl(
      const Hash& manifestNode,
      const Hash& edenTreeID,
      RelativePathPiece path,
      std::shared_ptr<LocalStore::WriteBatch> writeBatch);
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
#endif

  folly::Future<Hash> importManifest(Hash commitId);

  folly::Future<Hash> importFlatManifest(Hash commitId);

  LocalStore* localStore_{nullptr};
  // A set of threads owning HgImporter instances
  std::unique_ptr<folly::Executor> importThreadPool_;
  // The main server thread pool; we push the Futures back into
  // this pool to run their completion code to avoid clogging
  // the importer pool. Queuing in this pool can never block (which would risk
  // deadlock) or throw an exception when full (which would incorrectly fail the
  // load).
  folly::Executor* serverThreadPool_;
#if EDEN_HAVE_HG_TREEMANIFEST
  // These DatapackStore objects are never referenced once UnionDatapackStore
  // is allocated. They are here solely so their lifetime persists while the
  // UnionDatapackStore is alive.
  std::vector<std::unique_ptr<DatapackStore>> dataPackStores_;
  std::unique_ptr<folly::Synchronized<UnionDatapackStore>> unionStore_;

#ifndef EDEN_WIN_NOMONONOKE
  std::unique_ptr<MononokeBackingStore> mononoke_;
#endif // EDEN_WIN_NOMONONOKE
#endif // EDEN_HAVE_HG_TREEMANIFEST
};
} // namespace eden
} // namespace facebook
