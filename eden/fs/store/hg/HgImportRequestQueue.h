/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <condition_variable>
#include <mutex>
#include <vector>

#include <folly/Try.h>
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "folly/futures/Future.h"

namespace facebook::eden {

class ReloadableConfig;

class HgImportRequestQueue {
 public:
  explicit HgImportRequestQueue(std::shared_ptr<ReloadableConfig> config)
      : config_(std::move(config)) {}

  /**
   * Puts an item into the queue.
   */
  void enqueue(HgImportRequest request);

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
      const HgProxyHash& id,
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

  template <typename T>
  std::optional<folly::Future<std::unique_ptr<T>>> checkImportInProgress(
      const HgProxyHash& id,
      ImportPriority priority) {
    auto state = state_.lock();
    auto import = state->requestTracker.find(id);
    if (import != state->requestTracker.end()) {
      // Make empty promise, insert it into the vector, and return a
      // Future made from the reference to that promise in the map.
      auto promise = folly::Promise<std::unique_ptr<T>>();

      bool realRequest = false;
      std::vector<folly::Promise<std::unique_ptr<T>>>* promises;

      if constexpr (std::is_same_v<T, Tree>) {
        auto* treeImport =
            import->second->getRequest<HgImportRequest::TreeImport>();
        treeImport->promises.emplace_back(std::move(promise));
        realRequest = treeImport->realRequest;
        promises = &treeImport->promises;
      } else {
        static_assert(
            std::is_same_v<T, Blob>,
            "checkImportInProgress can only be called with a Tree or Blob types");
        auto* blobImport =
            import->second->getRequest<HgImportRequest::BlobImport>();
        blobImport->promises.emplace_back(std::move(promise));
        realRequest = blobImport->realRequest;
        promises = &blobImport->promises;
      }

      // This should always be valid since we insert a dummy request when we see
      // the first import request
      if (import->second->getPriority() < priority) {
        import->second->setPriority(priority);

        // Only do this while the request is not a dummy request
        if (realRequest) {
          std::make_heap(
              state->queue.begin(),
              state->queue.end(),
              [](std::shared_ptr<HgImportRequest> lhs,
                 std::shared_ptr<HgImportRequest> rhs) {
                return (*lhs) < (*rhs);
              });
        }
      }

      return std::make_optional(promises->back().getFuture());

    } else {
      // Insert a dummy request into the requestTracker to keep track of the
      // priorities we've set for the corresponding id
      if constexpr (std::is_same_v<T, Tree>) {
        state->requestTracker[id] = std::make_shared<HgImportRequest>(
            HgImportRequest::TreeImport{kEmptySha1, id, true, false},
            priority,
            folly::Promise<HgImportRequest::TreeImport::Response>{});
      } else {
        static_assert(
            std::is_same_v<T, Blob>,
            "checkImportInProgress can only be called with a Tree or Blob types");
        state->requestTracker[id] = std::make_shared<HgImportRequest>(
            HgImportRequest::BlobImport{kEmptySha1, id, false},
            priority,
            folly::Promise<HgImportRequest::BlobImport::Response>{});
      }

      return std::nullopt;
    }
  }

 private:
  HgImportRequestQueue(HgImportRequestQueue&&) = delete;
  HgImportRequestQueue& operator=(HgImportRequestQueue&&) = delete;

  struct State {
    bool running = true;
    std::vector<std::shared_ptr<HgImportRequest>> queue;

    /*
     * Map of a HgProxyHash to an element in the queue
     */
    std::unordered_map<HgProxyHash, std::shared_ptr<HgImportRequest>>
        requestTracker;
  };
  std::shared_ptr<ReloadableConfig> config_;
  folly::Synchronized<State, std::mutex> state_;
  std::condition_variable queueCV_;
};

} // namespace facebook::eden
