/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgImportRequestQueue.h"
#include <folly/MapUtil.h>
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

template <typename Ret, typename ImportType>
folly::Future<Ret> HgImportRequestQueue::enqueue(
    std::shared_ptr<HgImportRequest> request) {
  auto state = state_.lock();

  std::vector<std::shared_ptr<HgImportRequest>>* queue;
  if constexpr (std::is_same_v<ImportType, HgImportRequest::BlobImport>) {
    queue = &state->blobQueue;
  } else {
    static_assert(std::is_same_v<ImportType, HgImportRequest::TreeImport>);
    queue = &state->treeQueue;
  }

  const auto& hash = request->getRequest<ImportType>()->hash;
  if (auto* existingRequestPtr = folly::get_ptr(state->requestTracker, hash)) {
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
          queue->begin(),
          queue->end(),
          [](const std::shared_ptr<HgImportRequest>& lhs,
             const std::shared_ptr<HgImportRequest>& rhs) {
            return (*lhs) < (*rhs);
          });
    }

    return std::move(future).toUnsafeFuture();
  }

  queue->emplace_back(request);
  auto promise = request->getPromise<Ret>();

  state->requestTracker.emplace(hash, std::move(request));

  std::push_heap(
      queue->begin(),
      queue->end(),
      [](const std::shared_ptr<HgImportRequest>& lhs,
         const std::shared_ptr<HgImportRequest>& rhs) {
        return (*lhs) < (*rhs);
      });

  queueCV_.notify_one();

  return promise->getFuture();
}

std::vector<std::shared_ptr<HgImportRequest>>
HgImportRequestQueue::combineAndClearRequestQueues() {
  auto state = state_.lock();
  auto treeQSz = state->treeQueue.size();
  auto blobQSz = state->blobQueue.size();
  XLOGF(
      DBG5,
      "combineAndClearRequestQueues: tree queue size = {}, blob queue size = {}",
      treeQSz,
      blobQSz);
  auto res = std::move(state->treeQueue);
  res.insert(
      res.end(),
      std::make_move_iterator(state->blobQueue.begin()),
      std::make_move_iterator(state->blobQueue.end()));
  state->treeQueue.clear();
  state->blobQueue.clear();
  XCHECK_EQ(res.size(), treeQSz + blobQSz);
  return res;
}

std::vector<std::shared_ptr<HgImportRequest>> HgImportRequestQueue::dequeue() {
  size_t count;
  std::vector<std::shared_ptr<HgImportRequest>>* queue = nullptr;

  auto state = state_.lock();
  while (true) {
    if (!state->running) {
      state->treeQueue.clear();
      state->blobQueue.clear();
      return std::vector<std::shared_ptr<HgImportRequest>>();
    }

    auto highestPriority = ImportPriority::minimumValue();

    // Trees have a higher priority than blobs, thus check the queues in that
    // order.  The reason for trees having a higher priority is due to trees
    // allowing a higher fan-out and thus increasing concurrency of fetches
    // which translate onto a higher overall throughput.
    if (!state->treeQueue.empty()) {
      count = config_->getEdenConfig()->importBatchSizeTree.getValue();
      highestPriority = state->treeQueue.front()->getPriority();
      queue = &state->treeQueue;
    }

    if (!state->blobQueue.empty()) {
      auto priority = state->blobQueue.front()->getPriority();
      if (!queue || priority > highestPriority) {
        queue = &state->blobQueue;
        count = config_->getEdenConfig()->importBatchSize.getValue();
        highestPriority = priority;
      }
    }

    if (queue) {
      break;
    } else {
      queueCV_.wait(state.as_lock());
    }
  }

  count = std::min(count, queue->size());
  std::vector<std::shared_ptr<HgImportRequest>> result;
  result.reserve(count);
  for (size_t i = 0; i < count; i++) {
    std::pop_heap(
        queue->begin(),
        queue->end(),
        [](const std::shared_ptr<HgImportRequest>& lhs,
           const std::shared_ptr<HgImportRequest>& rhs) {
          return (*lhs) < (*rhs);
        });

    result.emplace_back(std::move(queue->back()));
    queue->pop_back();
  }

  return result;
}

} // namespace facebook::eden
