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
#include "eden/fs/store/hg/SaplingImportRequest.h"

namespace facebook::eden {

template <typename T>
class ImmediateFuture;
class ReloadableConfig;

class SaplingImportRequestQueue {
 public:
  explicit SaplingImportRequestQueue(std::shared_ptr<ReloadableConfig> config)
      : config_(std::move(config)) {}

  /**
   * Enqueue a blob request to the queue.
   *
   * Return a future that will complete when the blob request completes.
   */
  ImmediateFuture<BlobPtr> enqueueBlob(
      std::shared_ptr<SaplingImportRequest> request);

  /**
   * Enqueue a tree request to the queue.
   *
   * Return a future that will complete when the blob request completes.
   */
  ImmediateFuture<TreePtr> enqueueTree(
      std::shared_ptr<SaplingImportRequest> request);

  /**
   * Enqueue an aux data request to the queue.
   *
   * Return a future that will complete when the aux data request completes.
   */
  ImmediateFuture<BlobMetadataPtr> enqueueBlobMeta(
      std::shared_ptr<SaplingImportRequest> request);

  /**
   * Returns a list of requests from the queue. It returns an empty list while
   * the queue is being destructed. This function will block when there is no
   * item available in the queue.
   *
   * All requests in the vector are guaranteed to be the same type.
   * The number of the returned requests is controlled by `import-batch-size*`
   * options in the config. It may have fewer requests than configured.
   */
  std::vector<std::shared_ptr<SaplingImportRequest>> dequeue();

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
      folly::Try<std::shared_ptr<const T>>& importTry);

  /**
   * Combines all requests into 1 vec and clears the contents of the originals.
   */
  std::vector<std::shared_ptr<SaplingImportRequest>>
  combineAndClearRequestQueues();

 private:
  /**
   * Puts an item into the queue.
   */
  template <typename T, typename ImportType>
  ImmediateFuture<std::shared_ptr<const T>> enqueue(
      std::shared_ptr<SaplingImportRequest> request);

  SaplingImportRequestQueue(SaplingImportRequestQueue&&) = delete;
  SaplingImportRequestQueue& operator=(SaplingImportRequestQueue&&) = delete;

  struct ImportQueue {
    std::vector<std::shared_ptr<SaplingImportRequest>> queue;

    /**
     * Map of a ObjectId to an element in the queue. Any changes to this type
     * can have a significant effect on EdenFS performance and thus changes to
     * it needs to be carefully studied and measured. The
     * store/hg/tests/SaplingImportRequestQueueBenchmark.cpp is a good way to
     * measure the potential performance impact.
     */
    folly::F14FastMap<ObjectId, std::shared_ptr<SaplingImportRequest>>
        requestTracker;
  };

  struct State {
    bool running = true;
    ImportQueue treeQueue;
    ImportQueue blobQueue;
    ImportQueue blobMetaQueue;
  };

  /**
   * Short-hand to map the request type to the appropriate request tracker map.
   */
  template <typename T>
  SaplingImportRequestQueue::ImportQueue* getImportQueue(
      folly::Synchronized<SaplingImportRequestQueue::State, std::mutex>::
          LockedPtr& state);

  std::shared_ptr<ReloadableConfig> config_;
  folly::Synchronized<State, std::mutex> state_;
  std::condition_variable queueCV_;
};

template <typename T>
void SaplingImportRequestQueue::markImportAsFinished(
    const ObjectId& id,
    folly::Try<std::shared_ptr<const T>>& importTry) {
  std::shared_ptr<SaplingImportRequest> import;
  {
    auto state = state_.lock();

    auto& requestTracker = getImportQueue<T>(state)->requestTracker;
    auto importReq = requestTracker.find(id);
    if (importReq != requestTracker.end()) {
      import = std::move(importReq->second);
      requestTracker.erase(importReq);
    }
  }

  if (!import) {
    return;
  }

  std::vector<folly::Promise<std::shared_ptr<T>>>* promises;

  if constexpr (std::is_same_v<T, const Tree>) {
    auto* treeImport = import->getRequest<SaplingImportRequest::TreeImport>();
    promises = &treeImport->promises;
  } else if constexpr (std::is_same_v<T, const BlobMetadata>) {
    auto* blobMetaImport =
        import->getRequest<SaplingImportRequest::BlobMetaImport>();
    promises = &blobMetaImport->promises;
  } else {
    static_assert(
        std::is_same_v<T, const Blob>,
        "markImportAsFinished can only be called with Tree, Blob or BlobMetadata types");
    auto* blobImport = import->getRequest<SaplingImportRequest::BlobImport>();
    promises = &blobImport->promises;
  }

  if (importTry.hasValue()) {
    auto& importValue = importTry.value();
    for (auto& promise : (*promises)) {
      // If we find the id in the map, loop through all of the associated
      // Promises and fulfill them with the obj.
      promise.setValue(importValue);
    }
  } else {
    // If we find the id in the map, loop through all of the associated
    // Promises and fulfill them with the exception
    for (auto& promise : (*promises)) {
      promise.setException(importTry.exception());
    }
  }
}

template <typename T>
SaplingImportRequestQueue::ImportQueue*
SaplingImportRequestQueue::getImportQueue(folly::Synchronized<
                                          SaplingImportRequestQueue::State,
                                          std::mutex>::LockedPtr& state) {
  if constexpr (std::is_same_v<T, const Tree>) {
    return &state->treeQueue;
  } else if constexpr (std::is_same_v<T, const BlobMetadata>) {
    return &state->blobMetaQueue;
  } else {
    static_assert(
        std::is_same_v<T, const Blob>,
        "getImportQueue can only be called with Tree, Blob or BlobMetadata types");
    return &state->blobQueue;
  }
}
} // namespace facebook::eden
