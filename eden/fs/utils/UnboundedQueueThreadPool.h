/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/executors/CPUThreadPoolExecutor.h>

namespace facebook {
namespace eden {

/**
 * A thread pool Executor that is guaranteed to never block nor throw (except
 * OOM) from `add()`.
 */
class UnboundedQueueThreadPool : public folly::CPUThreadPoolExecutor {
 public:
  explicit UnboundedQueueThreadPool(
      size_t threadCount,
      folly::StringPiece threadNamePrefix);
};

} // namespace eden
} // namespace facebook
