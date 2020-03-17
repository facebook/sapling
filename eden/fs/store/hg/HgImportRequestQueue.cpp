/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgImportRequestQueue.h"

#include <folly/futures/Future.h>
#include <algorithm>

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

    state->queue.emplace_back(std::move(request));
    std::push_heap(state->queue.begin(), state->queue.end());
  }

  queueCV_.notify_one();
}

std::optional<HgImportRequest> HgImportRequestQueue::dequeue() {
  auto state = state_.lock();

  while (state->running && state->queue.empty()) {
    queueCV_.wait(state.getUniqueLock());
  }

  if (!state->running) {
    state->queue.clear();
    return std::nullopt;
  }

  std::pop_heap(state->queue.begin(), state->queue.end());

  auto request = std::move(state->queue.back());
  state->queue.pop_back();

  return std::move(request);
}
} // namespace eden
} // namespace facebook
