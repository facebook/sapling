/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/UnboundedQueueThreadPool.h"

#include <folly/executors/task_queue/UnboundedBlockingQueue.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>

namespace facebook {
namespace eden {

UnboundedQueueThreadPool::UnboundedQueueThreadPool(
    size_t threadCount,
    folly::StringPiece threadNamePrefix)
    : folly::CPUThreadPoolExecutor(
          threadCount,
          std::make_unique<folly::UnboundedBlockingQueue<
              folly::CPUThreadPoolExecutor::CPUTask>>(),
          std::make_unique<folly::NamedThreadFactory>(threadNamePrefix)) {}

} // namespace eden
} // namespace facebook
