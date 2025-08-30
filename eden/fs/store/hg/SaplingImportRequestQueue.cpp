/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/SaplingImportRequestQueue.h"
#include <folly/MapUtil.h>
#include <algorithm>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/fs/config/ReloadableConfig.h"

namespace facebook::eden {

void SaplingImportRequestQueue::stop() {
  auto state = state_.lock();
  if (state->running) {
    state->running = false;
    queueCV_.notify_all();
  }
}

ImmediateFuture<BlobPtr> SaplingImportRequestQueue::enqueueBlob(
    std::shared_ptr<SaplingImportRequest> request) {
  return enqueue<Blob, SaplingImportRequest::BlobImport>(std::move(request));
}

ImmediateFuture<TreePtr> SaplingImportRequestQueue::enqueueTree(
    std::shared_ptr<SaplingImportRequest> request) {
  return enqueue<Tree, SaplingImportRequest::TreeImport>(std::move(request));
}

ImmediateFuture<BlobAuxDataPtr> SaplingImportRequestQueue::enqueueBlobAux(
    std::shared_ptr<SaplingImportRequest> request) {
  return enqueue<BlobAuxData, SaplingImportRequest::BlobAuxImport>(
      std::move(request));
}

ImmediateFuture<TreeAuxDataPtr> SaplingImportRequestQueue::enqueueTreeAux(
    std::shared_ptr<SaplingImportRequest> request) {
  return enqueue<TreeAuxData, SaplingImportRequest::TreeAuxImport>(
      std::move(request));
}

template <typename T, typename ImportType>
ImmediateFuture<std::shared_ptr<const T>> SaplingImportRequestQueue::enqueue(
    std::shared_ptr<SaplingImportRequest> request) {
  auto state = state_.lock();
  auto* importQueue = getImportQueue<const T>(state);
  auto* requestQueue = &importQueue->queue;

  const auto& id = request->getRequest<ImportType>()->id;
  if (auto* existingRequestPtr =
          folly::get_ptr(importQueue->requestTracker, id)) {
    auto& existingRequest = *existingRequestPtr;
    auto* trackedImport = existingRequest->template getRequest<ImportType>();

    auto [promise, future] =
        folly::makePromiseContract<std::shared_ptr<const T>>();
    trackedImport->promises.emplace_back(std::move(promise));

    if (existingRequest->getPriority() < request->getPriority()) {
      existingRequest->setPriority(request->getPriority());

      // Since the new request has a higher priority than the already present
      // one, we need to re-order the heap.
      //
      // TODO(xavierd): this has a O(n) complexity, and enqueuing tons of
      // duplicated requests will thus lead to a quadratic complexity.
      std::make_heap(
          requestQueue->begin(),
          requestQueue->end(),
          [](const std::shared_ptr<SaplingImportRequest>& lhs,
             const std::shared_ptr<SaplingImportRequest>& rhs) {
            return (*lhs) < (*rhs);
          });
    }

    return std::move(future);
  }

  requestQueue->emplace_back(request);
  auto promise = request->getPromise<std::shared_ptr<const T>>();

  importQueue->requestTracker.emplace(id, std::move(request));

  std::push_heap(
      requestQueue->begin(),
      requestQueue->end(),
      [](const std::shared_ptr<SaplingImportRequest>& lhs,
         const std::shared_ptr<SaplingImportRequest>& rhs) {
        return (*lhs) < (*rhs);
      });

  queueCV_.notify_one();

  return promise->getSemiFuture();
}

std::vector<std::shared_ptr<SaplingImportRequest>>
SaplingImportRequestQueue::combineAndClearRequestQueues() {
  auto state = state_.lock();
  auto treeQSz = state->treeQueue.queue.size();
  auto blobQSz = state->blobQueue.queue.size();
  auto blobAuxQSz = state->blobAuxQueue.queue.size();
  auto treeAuxQSz = state->treeAuxQueue.queue.size();
  XLOGF(
      DBG5,
      "combineAndClearRequestQueues: tree queue size = {}, blob queue size = {}, blob aux data queue size = {}, tree aux data queue size = {}",
      treeQSz,
      blobQSz,
      blobAuxQSz,
      treeAuxQSz);
  auto res = std::move(state->treeQueue.queue);
  res.insert(
      res.end(),
      std::make_move_iterator(state->blobQueue.queue.begin()),
      std::make_move_iterator(state->blobQueue.queue.end()));
  res.insert(
      res.end(),
      std::make_move_iterator(state->blobAuxQueue.queue.begin()),
      std::make_move_iterator(state->blobAuxQueue.queue.end()));
  res.insert(
      res.end(),
      std::make_move_iterator(state->treeAuxQueue.queue.begin()),
      std::make_move_iterator(state->treeAuxQueue.queue.end()));
  state->treeQueue.queue.clear();
  state->blobQueue.queue.clear();
  state->blobAuxQueue.queue.clear();
  state->treeAuxQueue.queue.clear();
  XCHECK_EQ(res.size(), treeQSz + blobQSz + blobAuxQSz + treeAuxQSz);
  return res;
}

std::vector<std::shared_ptr<SaplingImportRequest>>
SaplingImportRequestQueue::dequeue() {
  size_t count;
  std::vector<std::shared_ptr<SaplingImportRequest>>* queue = nullptr;

  auto state = state_.lock();
  while (true) {
    if (!state->running) {
      state->treeQueue.queue.clear();
      state->blobQueue.queue.clear();
      state->blobAuxQueue.queue.clear();
      state->treeAuxQueue.queue.clear();
      return std::vector<std::shared_ptr<SaplingImportRequest>>();
    }

    auto highestPriority = ImportPriority::minimumValue();

    // Trees have a higher priority than blobs, thus check the queues in that
    // order.  The reason for trees having a higher priority is due to trees
    // allowing a higher fan-out and thus increasing concurrency of fetches
    // which translate onto a higher overall throughput.
    if (!state->treeQueue.queue.empty()) {
      count = config_->getEdenConfig()->importBatchSizeTree.getValue();
      highestPriority = state->treeQueue.queue.front()->getPriority();
      queue = &state->treeQueue.queue;
    }

    if (!state->treeAuxQueue.queue.empty()) {
      auto priority = state->treeAuxQueue.queue.front()->getPriority();
      if (!queue || priority > highestPriority) {
        queue = &state->treeAuxQueue.queue;
        count = config_->getEdenConfig()->importBatchSizeTreeMeta.getValue();
        highestPriority = priority;
      }
    }

    if (!state->blobAuxQueue.queue.empty()) {
      auto priority = state->blobAuxQueue.queue.front()->getPriority();
      if (!queue || priority > highestPriority) {
        queue = &state->blobAuxQueue.queue;
        count = config_->getEdenConfig()->importBatchSizeBlobMeta.getValue();
        highestPriority = priority;
      }
    }

    if (!state->blobQueue.queue.empty()) {
      auto priority = state->blobQueue.queue.front()->getPriority();
      if (!queue || priority > highestPriority) {
        queue = &state->blobQueue.queue;
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
  std::vector<std::shared_ptr<SaplingImportRequest>> result;
  result.reserve(count);
  for (size_t i = 0; i < count; i++) {
    std::pop_heap(
        queue->begin(),
        queue->end(),
        [](const std::shared_ptr<SaplingImportRequest>& lhs,
           const std::shared_ptr<SaplingImportRequest>& rhs) {
          return (*lhs) < (*rhs);
        });

    result.emplace_back(std::move(queue->back()));
    queue->pop_back();
  }

  return result;
}

} // namespace facebook::eden
