/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
    std::unique_ptr<folly::ManualExecutor> executor)
    : executor_{std::move(executor)} {}

} // namespace eden
} // namespace facebook
