/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Executor.h>
#include <folly/Range.h>

namespace folly {
class ManualExecutor;
}

namespace facebook {
namespace eden {

/**
 * An Executor that is guaranteed to never block, nor throw (except OOM), nor
 * execute inline from `add()`.
 *
 * Parts of Eden rely on queuing a function to be non-blocking for deadlock
 * safety.
 */
class UnboundedQueueExecutor : public folly::Executor {
 public:
  /**
   * Instantiates with a folly::CPUThreadPoolExecutor with the given threadCount
   * and threadNamePrefix but with an unlimited queue.
   */
  explicit UnboundedQueueExecutor(
      size_t threadCount,
      folly::StringPiece threadNamePrefix);

  /**
   * ManualExecutors are unbounded too.
   *
   * Used primarily for tests.
   */
  explicit UnboundedQueueExecutor(
      std::shared_ptr<folly::ManualExecutor> executor);

  UnboundedQueueExecutor(const UnboundedQueueExecutor&) = delete;
  UnboundedQueueExecutor& operator=(const UnboundedQueueExecutor&) = delete;
  UnboundedQueueExecutor(UnboundedQueueExecutor&&) = delete;
  UnboundedQueueExecutor& operator=(UnboundedQueueExecutor&&) = delete;

  void add(folly::Func func) override {
    executor_->add(std::move(func));
  }

 private:
  std::shared_ptr<folly::Executor> executor_;
};

} // namespace eden
} // namespace facebook
