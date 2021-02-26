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

namespace facebook {
namespace eden {

class HgImportRequestQueue {
 public:
  explicit HgImportRequestQueue() {}

  /*
   * Puts an item into the queue.
   */
  void enqueue(HgImportRequest request);

  /*
   * Returns a list of requests from the queue. It returns an empty list while
   * the queue is being destructed. This function will block when there is no
   * item available in the queue.
   *
   * The returned vector may have fewer requests than it requested, and all
   * requests in the vector are guaranteed to be the same type.
   */
  std::vector<std::shared_ptr<HgImportRequest>> dequeue(size_t count);

  void stop();

  /* ====== De-duplication methods ====== */
  template <typename T>
  void markImportAsFinished(
      const HgProxyHash& id,
      folly::Try<std::unique_ptr<T>>& importTry) {
    auto state = state_.lock();

    auto importReq = state->requestTracker.find(id);
    if (importReq != state->requestTracker.end()) {
      auto foundImport = std::move(importReq->second);
      state->requestTracker.erase(importReq);

      std::vector<folly::Promise<std::unique_ptr<T>>>* promises;

      if constexpr (std::is_same_v<T, Tree>) {
        auto* treeImport =
            foundImport->getRequest<HgImportRequest::TreeImport>();
        promises = &treeImport->promises;
      } else if constexpr (std::is_same_v<T, Blob>) {
        auto* blobImport =
            foundImport->getRequest<HgImportRequest::BlobImport>();
        promises = &blobImport->promises;
      } else {
        // function called with unsupported type
        throw std::logic_error(
            "Attempting to call markImportAsFinished with unsupported type");
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
      } else if constexpr (std::is_same_v<T, Blob>) {
        auto* blobImport =
            import->second->getRequest<HgImportRequest::BlobImport>();
        blobImport->promises.emplace_back(std::move(promise));
        realRequest = blobImport->realRequest;
        promises = &blobImport->promises;
      } else {
        // function called with unsupported type
        throw std::logic_error(
            "Attempting to call checkImportInProgress with unsupported type");
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
      } else if constexpr (std::is_same_v<T, Blob>) {
        state->requestTracker[id] = std::make_shared<HgImportRequest>(
            HgImportRequest::BlobImport{kEmptySha1, id, false},
            priority,
            folly::Promise<HgImportRequest::BlobImport::Response>{});
      } else {
        // function called with unsupported type
        throw std::logic_error(
            "Attempting to call checkImportInProgress with unsupported type");
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

  folly::Synchronized<State, std::mutex> state_;
  std::condition_variable queueCV_;
};

} // namespace eden
} // namespace facebook
