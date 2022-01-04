/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/EdenTaskQueue.h"

namespace facebook::eden {

folly::BlockingQueueAddResult EdenTaskQueue::add(
    folly::CPUThreadPoolExecutor::CPUTask item) {
  queue_.enqueue(std::move(item));
  return sem_.post();
}

folly::CPUThreadPoolExecutor::CPUTask EdenTaskQueue::take() {
  sem_.wait();
  folly::CPUThreadPoolExecutor::CPUTask res;
  queue_.dequeue(res);
  return res;
}

folly::Optional<folly::CPUThreadPoolExecutor::CPUTask>
EdenTaskQueue::try_take_for(std::chrono::milliseconds time) {
  if (!sem_.try_wait_for(time)) {
    return folly::none;
  }
  folly::CPUThreadPoolExecutor::CPUTask res;
  queue_.dequeue(res);
  return res;
}

size_t EdenTaskQueue::size() {
  return queue_.size();
}

} // namespace facebook::eden
