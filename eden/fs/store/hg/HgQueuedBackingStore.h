/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <memory>
#include <vector>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImportRequestQueue.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"

namespace facebook {
namespace eden {

class BackingStoreLogger;
class ReloadableConfig;
class HgBackingStore;
class LocalStore;
class EdenStats;
class HgImportRequest;

constexpr uint8_t kNumberHgQueueWorker = 8;

/**
 * An Hg backing store implementation that will put incoming blob/tree import
 * requests into a job queue, then a pool of workers will work on fulfilling
 * these requests via different methods (reading from hgcache, Mononoke,
 * debugimporthelper, etc.).
 */
class HgQueuedBackingStore : public BackingStore {
 public:
  HgQueuedBackingStore(
      std::shared_ptr<LocalStore> localStore,
      std::shared_ptr<EdenStats> stats,
      std::unique_ptr<HgBackingStore> backingStore,
      std::shared_ptr<ReloadableConfig> config,
      std::unique_ptr<BackingStoreLogger> logger,
      uint8_t numberThreads = kNumberHgQueueWorker);

  ~HgQueuedBackingStore() override;

  folly::SemiFuture<std::unique_ptr<Tree>> getTree(
      const Hash& id,
      ObjectFetchContext& context) override;
  folly::SemiFuture<std::unique_ptr<Blob>> getBlob(
      const Hash& id,
      ObjectFetchContext& context) override;

  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) override;

  FOLLY_NODISCARD virtual folly::SemiFuture<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids,
      ObjectFetchContext& context) override;

  HgBackingStore* getHgBackingStore() const {
    return backingStore_.get();
  }

  /**
   * calculates `metric` for `object` imports that are `stage`.
   *    ex. HgQueuedBackingStore::getImportMetrics(
   *          RequestMetricsScope::HgImportStage::PENDING,
   *          RequestMetricsScope::HgImportObject::BLOB,
   *          RequestMetricsScope::Metric::COUNT,
   *        )
   *    calculates the number of blob imports that are pending
   */
  size_t getImportMetric(
      RequestMetricsScope::RequestStage stage,
      HgBackingStore::HgImportObject object,
      RequestMetricsScope::RequestMetric metric) const;

 private:
  // Forbidden copy constructor and assignment operator
  HgQueuedBackingStore(const HgQueuedBackingStore&) = delete;
  HgQueuedBackingStore& operator=(const HgQueuedBackingStore&) = delete;

  void processBlobImportRequests(std::vector<HgImportRequest>&& requests);
  void processTreeImportRequests(std::vector<HgImportRequest>&& requests);
  void processPrefetchRequests(std::vector<HgImportRequest>&& requests);

  /**
   * The worker runloop function.
   */
  void processRequest();

  /**
   * Logs a backing store fetch to scuba if the path being fetched is
   * in the configured paths to log. If `identifer` is a RelativePathPiece this
   * will be used as the "path being fetched". If the `identifer` is a Hash
   * then this will look up the path with HgProxyHash to be used as the
   * "path being fetched"
   */
  void logBackingStoreFetch(
      ObjectFetchContext& context,
      std::variant<RelativePathPiece, Hash> identifer);

  /**
   * gets the watches timing `object` imports that are `stage`
   *    ex. HgQueuedBackingStore::getImportWatches(
   *          RequestMetricsScope::HgImportStage::PENDING,
   *          HgBackingStore::HgImportObject::BLOB,
   *        )
   *    gets the watches timing blob imports that are pending
   */
  RequestMetricsScope::LockedRequestWatchList& getImportWatches(
      RequestMetricsScope::RequestStage stage,
      HgBackingStore::HgImportObject object) const;

  /**
   * Gets the watches timing pending `object` imports
   *   ex. HgBackingStore::getPendingImportWatches(
   *          HgBackingStore::HgImportObject::BLOB,
   *        )
   *    gets the watches timing pending blob imports
   */
  RequestMetricsScope::LockedRequestWatchList& getPendingImportWatches(
      HgBackingStore::HgImportObject object) const;

  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<EdenStats> stats_;

  /**
   * Reference to the eden config, may be a null pointer in unit tests.
   */
  std::shared_ptr<ReloadableConfig> config_;

  std::unique_ptr<HgBackingStore> backingStore_;

  /**
   * The import request queue. This queue is unbounded. This queue
   * implementation will ensure enqueue operation never blocks.
   */
  HgImportRequestQueue queue_;

  /**
   * The worker thread pool. These threads will be running `processRequest`
   * forever to process incoming import requests
   */
  std::vector<std::thread> threads_;

  /**
   * Logger for backing store imports
   */
  std::unique_ptr<BackingStoreLogger> logger_;

  // Track metrics for queued imports
  mutable RequestMetricsScope::LockedRequestWatchList pendingImportBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList pendingImportTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      pendingImportPrefetchWatches_;
};

} // namespace eden
} // namespace facebook
