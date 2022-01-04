/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/concurrency/DynamicBoundedQueue.h>
#include <folly/executors/CPUThreadPoolExecutor.h>

namespace facebook::eden {

/**
 * Task queue that can be used to hold work needing to be processed.
 *
 * This is backed by a DMPMCQueue.
 */
class EdenTaskQueue
    : public folly::BlockingQueue<folly::CPUThreadPoolExecutor::CPUTask> {
 public:
  explicit EdenTaskQueue(uint64_t maxInflightRequests)
      : queue_(folly::DMPMCQueue<folly::CPUThreadPoolExecutor::CPUTask, true>{
            maxInflightRequests}) {}

  folly::BlockingQueueAddResult add(
      folly::CPUThreadPoolExecutor::CPUTask item) override;

  folly::CPUThreadPoolExecutor::CPUTask take() override;

  folly::Optional<folly::CPUThreadPoolExecutor::CPUTask> try_take_for(
      std::chrono::milliseconds time) override;

  size_t size() override;

 private:
  folly::LifoSem sem_;
  folly::DMPMCQueue<folly::CPUThreadPoolExecutor::CPUTask, true> queue_;
};

} // namespace facebook::eden
