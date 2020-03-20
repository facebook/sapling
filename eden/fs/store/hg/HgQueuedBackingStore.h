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

  HgBackingStore* getHgBackingStore() const {
    return backingStore_.get();
  }

 private:
  // Forbidden copy constructor and assignment operator
  HgQueuedBackingStore(const HgQueuedBackingStore&) = delete;
  HgQueuedBackingStore& operator=(const HgQueuedBackingStore&) = delete;

  /**
   * The worker runloop function.
   */
  void processRequest();

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
};

} // namespace eden
} // namespace facebook
