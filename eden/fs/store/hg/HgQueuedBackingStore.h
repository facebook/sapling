/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/executors/CPUThreadPoolExecutor.h>
#include <memory>

#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgImportRequestQueue.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"

namespace facebook {
namespace eden {

class ReloadableConfig;
class HgBackingStore;

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
      std::unique_ptr<HgBackingStore> backingStore,
      uint8_t numberThreads = kNumberHgQueueWorker);

  ~HgQueuedBackingStore() override;

  folly::SemiFuture<std::unique_ptr<Tree>> getTree(
      const Hash& id,
      ImportPriority priority = ImportPriority::kNormal()) override;
  folly::SemiFuture<std::unique_ptr<Blob>> getBlob(
      const Hash& id,
      ImportPriority priority = ImportPriority::kNormal()) override;

  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) override;

  FOLLY_NODISCARD virtual folly::SemiFuture<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& /*ids*/) override;

  HgBackingStore* getHgBackingStore() const {
    return backingStore_.get();
  }

  /**
   * stages of hg import that are tracked, these represent where an import is in
   * the import process (for example an import could be queued, fetching from
   * hg, or checking cache)
   */
  enum HgImportStage {
    // represents any import that has been requested but not yet completed
    // (imports in this stage could be in the queue, checking the cache, or
    // fetching data from hg
    PENDING,
    // represents imports that are currently fetching data from hg
    LIVE,
  };

  constexpr static std::array<HgImportStage, 2> hgImportStages{
      HgImportStage::PENDING,
      HgImportStage::LIVE};

  static std::string stringOfHgImportStage(HgImportStage stage);

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
      HgImportStage stage,
      HgBackingStore::HgImportObject object,
      RequestMetricsScope::RequestMetric metric) const;

 private:
  // Forbidden copy constructor and assignment operator
  HgQueuedBackingStore(const HgQueuedBackingStore&) = delete;
  HgQueuedBackingStore& operator=(const HgQueuedBackingStore&) = delete;

  /**
   * The worker runloop function.
   */
  void processRequest();

  /**
   * gets the watches timing `object` imports that are `stage`
   *    ex. HgQueuedBackingStore::getImportWatches(
   *          RequestMetricsScope::HgImportStage::PENDING,
   *          RequestMetricsScope::HgImportObject::BLOB,
   *        )
   *    gets the watches timing blob imports that are pending
   */
  RequestMetricsScope::LockedRequestWatchList& getImportWatches(
      HgImportStage stage,
      HgBackingStore::HgImportObject object) const;

  /**
   * Gets the watches timing pending `object` imports
   *   ex. HgBackingStore::getPendingImportWatches(
   *          RequestMetricsScope::HgImportObject::BLOB,
   *        )
   *    gets the watches timing pending blob imports
   */
  RequestMetricsScope::LockedRequestWatchList& getPendingImportWatches(
      HgBackingStore::HgImportObject object) const;

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

  // Track metrics for queued imports
  mutable RequestMetricsScope::LockedRequestWatchList pendingImportBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList pendingImportTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      pendingImportPrefetchWatches_;
};

} // namespace eden
} // namespace facebook
