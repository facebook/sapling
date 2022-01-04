/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <folly/Try.h>
#include <folly/container/F14Map.h>
#include <condition_variable>
#include <mutex>
#include <vector>
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "folly/futures/Future.h"

namespace facebook::eden {

class ReloadableConfig;

class HgImportRequestQueue {
 public:
  explicit HgImportRequestQueue(std::shared_ptr<ReloadableConfig> config)
      : config_(std::move(config)) {}

  /**
   * Enqueue a blob request to the queue.
   *
   * Return a future that will complete when the blob request completes.
   */
  folly::Future<std::unique_ptr<Blob>> enqueueBlob(
      std::shared_ptr<HgImportRequest> request);

  /**
   * Enqueue a tree request to the queue.
   *
   * Return a future that will complete when the blob request completes.
   */
  folly::Future<std::unique_ptr<Tree>> enqueueTree(
      std::shared_ptr<HgImportRequest> request);

  /**
   * Enqueue a prefetch request to the queue
   *
   * Return a future that will complete when the prefetch request
   * completes.
   */
  folly::Future<folly::Unit> enqueuePrefetch(
      std::shared_ptr<HgImportRequest> request);

  /**
   * Returns a list of requests from the queue. It returns an empty list while
   * the queue is being destructed. This function will block when there is no
   * item available in the queue.
   *
   * All requests in the vector are guaranteed to be the same type.
   * The number of the returned requests is controlled by `import-batch-size*`
   * options in the config. It may have fewer requests than configured.
   */
  std::vector<std::shared_ptr<HgImportRequest>> dequeue();

  /**
   * Destroy the queue.
   *
   * Intended to be called in the destructor of the owner of the queue as
   * subsequent enqueue will never be handled. Future dequeue calls will
   * return an empty list.
   */
  void stop();

  /* ====== De-duplication methods ====== */
  template <typename T>
  void markImportAsFinished(
      const ObjectId& id,
      folly::Try<std::unique_ptr<T>>& importTry) {
    std::shared_ptr<HgImportRequest> import;
    {
      auto state = state_.lock();

      auto importReq = state->requestTracker.find(id);
      if (importReq != state->requestTracker.end()) {
        import = std::move(importReq->second);
        state->requestTracker.erase(importReq);
      }
    }

    if (!import) {
      return;
    }

    std::vector<folly::Promise<std::unique_ptr<T>>>* promises;

    if constexpr (std::is_same_v<T, Tree>) {
      auto* treeImport = import->getRequest<HgImportRequest::TreeImport>();
      promises = &treeImport->promises;
    } else {
      static_assert(
          std::is_same_v<T, Blob>,
          "markImportAsFinished can only be called with Tree or Blob types");
      auto* blobImport = import->getRequest<HgImportRequest::BlobImport>();
      promises = &blobImport->promises;
    }

    if (importTry.hasValue()) {
      // If we find the id in the map, loop through all of the associated
      // Promises and fulfill them with the obj. We need to construct a
      // deep copy of the unique_ptr to fulfill the Promises
      for (auto& promise : (*promises)) {
        promise.setValue(std::make_unique<T>(*(importTry.value())));
      }
    } else {
      // If we find the id in the map, loop through all of the associated
      // Promises and fulfill them with the exception
      for (auto& promise : (*promises)) {
        promise.setException(importTry.exception());
      }
    }
  }

 private:
  /**
   * Puts an item into the queue.
   */
  template <typename Ret, typename ImportType>
  folly::Future<Ret> enqueue(std::shared_ptr<HgImportRequest> request);

  HgImportRequestQueue(HgImportRequestQueue&&) = delete;
  HgImportRequestQueue& operator=(HgImportRequestQueue&&) = delete;

  struct State {
    bool running = true;
    std::vector<std::shared_ptr<HgImportRequest>> treeQueue;
    std::vector<std::shared_ptr<HgImportRequest>> blobQueue;
    std::vector<std::shared_ptr<HgImportRequest>> prefetchQueue;

    /**
     * Map of a ObjectId to an element in the queue. Any changes to this type
     * can have a significant effect on EdenFS performance and thus changes to
     * it needs to be carefully studied and measured. The
     * benchmarks/hg_import_request_queue.cpp is a good way to measure the
     * potential performance impact.
     */
    folly::F14FastMap<ObjectId, std::shared_ptr<HgImportRequest>>
        requestTracker;
  };
  std::shared_ptr<ReloadableConfig> config_;
  folly::Synchronized<State, std::mutex> state_;
  std::condition_variable queueCV_;
};

} // namespace facebook::eden
