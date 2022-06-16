/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenCPUThreadPool.h"

#include <folly/portability/GFlags.h>

DEFINE_int32(num_eden_threads, 12, "the number of eden CPU worker threads");

namespace facebook::eden {

EdenCPUThreadPool::EdenCPUThreadPool()
    : UnboundedQueueExecutor(FLAGS_num_eden_threads, "EdenCPUThread") {}

} // namespace facebook::eden
