/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgImportRequestQueue.h"
#include <folly/futures/Future.h>
#include <algorithm>
#include "eden/fs/config/ReloadableConfig.h"

namespace facebook::eden {

void HgImportRequestQueue::stop() {
  auto state = state_.lock();
  if (state->running) {
    state->running = false;
    queueCV_.notify_all();
  }
}

folly::Future<std::unique_ptr<Blob>> HgImportRequestQueue::enqueueBlob(
    std::shared_ptr<HgImportRequest> request) {
  return enqueue<std::unique_ptr<Blob>, HgImportRequest::BlobImport>(
      std::move(request));
}

folly::Future<std::unique_ptr<Tree>> HgImportRequestQueue::enqueueTree(
    std::shared_ptr<HgImportRequest> request) {
  return enqueue<std::unique_ptr<Tree>, HgImportRequest::TreeImport>(
      std::move(request));
}

folly::Future<folly::Unit> HgImportRequestQueue::enqueuePrefetch(
    std::shared_ptr<HgImportRequest> request) {
  return enqueue<folly::Unit>(std::move(request));
}

template <typename Ret, typename ImportType>
folly::Future<Ret> HgImportRequestQueue::enqueue(
    std::shared_ptr<HgImportRequest> request) {
  auto state = state_.lock();

  if constexpr (!std::is_same_v<ImportType, void>) {
    const auto& proxyHash = request->getRequest<ImportType>()->proxyHash;

    if (auto* existingRequestPtr =
            folly::get_ptr(state->requestTracker, proxyHash)) {
      auto& existingRequest = *existingRequestPtr;
      auto* trackedImport = existingRequest->template getRequest<ImportType>();

      auto [promise, future] = folly::makePromiseContract<Ret>();
      trackedImport->promises.emplace_back(std::move(promise));

      if (existingRequest->getPriority() < request->getPriority()) {
        existingRequest->setPriority(request->getPriority());

        // Since the new request has a higher priority than the already present
        // one, we need to re-order the heap.
        //
        // TODO(xavierd): this has a O(n) complexity, and enqueing tons of
        // duplicated requests will thus lead to a quadratic complexity.
        std::make_heap(
            state->queue.begin(),
            state->queue.end(),
            [](const std::shared_ptr<HgImportRequest>& lhs,
               const std::shared_ptr<HgImportRequest>& rhs) {
              return (*lhs) < (*rhs);
            });
      }

      return std::move(future).toUnsafeFuture();
    }
  }

  state->queue.emplace_back(request);
  auto promise = request->getPromise<Ret>();

  if constexpr (!std::is_same_v<ImportType, void>) {
    const auto& proxyHash = request->getRequest<ImportType>()->proxyHash;
    state->requestTracker.emplace(proxyHash, std::move(request));
  }

  std::push_heap(
      state->queue.begin(),
      state->queue.end(),
      [](const std::shared_ptr<HgImportRequest>& lhs,
         const std::shared_ptr<HgImportRequest>& rhs) {
        return (*lhs) < (*rhs);
      });

  queueCV_.notify_one();

  return promise->getFuture();
}

std::vector<std::shared_ptr<HgImportRequest>> HgImportRequestQueue::dequeue() {
  auto state = state_.lock();

  while (state->running && state->queue.empty()) {
    queueCV_.wait(state.as_lock());
  }

  if (!state->running) {
    state->queue.clear();
    return std::vector<std::shared_ptr<HgImportRequest>>();
  }

  auto& queue = state->queue;

  std::vector<std::shared_ptr<HgImportRequest>> result;
  std::vector<std::shared_ptr<HgImportRequest>> putback;

  // The highest-pri request is the first element of the queue (a heap).
  size_t type = queue.front()->getType();
  size_t count = queue.front()->isType<HgImportRequest::TreeImport>()
      ? config_->getEdenConfig()->importBatchSizeTree.getValue()
      : config_->getEdenConfig()->importBatchSize.getValue();

  for (size_t i = 0; i < count * 3; i++) {
    if (queue.empty() || result.size() == count) {
      break;
    }

    std::pop_heap(
        queue.begin(),
        queue.end(),
        [](const std::shared_ptr<HgImportRequest>& lhs,
           const std::shared_ptr<HgImportRequest>& rhs) {
          return (*lhs) < (*rhs);
        });

    auto request = std::move(queue.back());
    queue.pop_back();

    if (type == request->getType()) {
      result.emplace_back(std::move(request));
    } else {
      putback.emplace_back(std::move(request));
    }
  }

  for (auto& item : putback) {
    queue.emplace_back(std::move(item));
    std::push_heap(
        queue.begin(),
        queue.end(),
        [](const std::shared_ptr<HgImportRequest>& lhs,
           const std::shared_ptr<HgImportRequest>& rhs) {
          return (*lhs) < (*rhs);
        });
  }

  return result;
}

} // namespace facebook::eden
