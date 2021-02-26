/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgImportRequestQueue.h"

#include <folly/futures/Future.h>
#include <algorithm>
#include <complex>

namespace facebook {
namespace eden {

void HgImportRequestQueue::stop() {
  auto state = state_.lock();
  if (state->running) {
    state->running = false;
    queueCV_.notify_all();
  }
}

void HgImportRequestQueue::enqueue(HgImportRequest request) {
  {
    auto state = state_.lock();

    if (!state->running) {
      // if the queue is stopped, no need to enqueue
      return;
    }

    state->queue.emplace_back(
        std::make_shared<HgImportRequest>(std::move(request)));
    auto& requestPtr = state->queue.back();

    // Put our request in the request tracker if the request not a
    // PrefetchRequest
    if (requestPtr->isType<HgImportRequest::BlobImport>()) {
      auto* blobImport = requestPtr->getRequest<HgImportRequest::BlobImport>();
      auto& proxyHash = blobImport->proxyHash;

      auto& trackedRequest = state->requestTracker[proxyHash];
      auto* trackedBlobImport =
          trackedRequest->getRequest<HgImportRequest::BlobImport>();

      // If we get multiple requests at once, it is possible that we call
      // checkImportInProgress multiple times before we enqueue the request. In
      // this case, we "send away" the duplicate requests, but we still keep
      // track of the highest priority we've seen. We need to update the
      // enqueued request's priority with the highest priority we've seen so far
      if (requestPtr->getPriority() < trackedRequest->getPriority()) {
        requestPtr->setPriority(trackedRequest->getPriority());
      }

      // std::move the vector of already generated promises from the dummy
      // request to the new "real" request. The dummy request collects promises
      // for duplicate requests that come in before we enqueue the first request
      blobImport->promises = std::move(trackedBlobImport->promises);
      trackedRequest = requestPtr;
    } else if (requestPtr->isType<HgImportRequest::TreeImport>()) {
      auto* treeImport = requestPtr->getRequest<HgImportRequest::TreeImport>();
      auto& proxyHash = treeImport->proxyHash;

      auto& trackedRequest = state->requestTracker[proxyHash];
      auto* trackedTreeImport =
          trackedRequest->getRequest<HgImportRequest::TreeImport>();

      if (requestPtr->getPriority() < trackedRequest->getPriority()) {
        requestPtr->setPriority(trackedRequest->getPriority());
      }

      treeImport->promises = std::move(trackedTreeImport->promises);
      trackedRequest = requestPtr;
    }

    std::push_heap(
        state->queue.begin(),
        state->queue.end(),
        [](const std::shared_ptr<HgImportRequest>& lhs,
           const std::shared_ptr<HgImportRequest>& rhs) {
          return (*lhs) < (*rhs);
        });
  }

  queueCV_.notify_one();
}

std::vector<std::shared_ptr<HgImportRequest>> HgImportRequestQueue::dequeue(
    size_t count) {
  auto state = state_.lock();

  while (state->running && state->queue.empty()) {
    queueCV_.wait(state.getUniqueLock());
  }

  if (!state->running) {
    state->queue.clear();
    return std::vector<std::shared_ptr<HgImportRequest>>();
  }

  auto& queue = state->queue;

  std::vector<std::shared_ptr<HgImportRequest>> result;
  std::vector<std::shared_ptr<HgImportRequest>> putback;
  std::optional<size_t> type;

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

    if (!type) {
      type = request->getType();
      result.emplace_back(std::move(request));
    } else {
      if (*type == request->getType()) {
        result.emplace_back(std::move(request));
      } else {
        putback.emplace_back(std::move(request));
      }
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
} // namespace eden
} // namespace facebook
