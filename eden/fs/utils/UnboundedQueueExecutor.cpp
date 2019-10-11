/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/UnboundedQueueExecutor.h"

#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/executors/task_queue/UnboundedBlockingQueue.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>

namespace facebook {
namespace eden {

UnboundedQueueExecutor::UnboundedQueueExecutor(
    size_t threadCount,
    folly::StringPiece threadNamePrefix)
    : executor_{std::make_unique<folly::CPUThreadPoolExecutor>(
          threadCount,
          std::make_unique<folly::UnboundedBlockingQueue<
              folly::CPUThreadPoolExecutor::CPUTask>>(),
          std::make_unique<folly::NamedThreadFactory>(threadNamePrefix))} {}

UnboundedQueueExecutor::UnboundedQueueExecutor(
    std::shared_ptr<folly::ManualExecutor> executor)
    : executor_{std::move(executor)} {}

} // namespace eden
} // namespace facebook
