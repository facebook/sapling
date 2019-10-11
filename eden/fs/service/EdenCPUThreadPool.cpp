/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenCPUThreadPool.h"

#include <gflags/gflags.h>

DEFINE_int32(num_eden_threads, 12, "the number of eden CPU worker threads");

namespace facebook {
namespace eden {

EdenCPUThreadPool::EdenCPUThreadPool()
    : UnboundedQueueExecutor(FLAGS_num_eden_threads, "EdenCPUThread") {}

} // namespace eden
} // namespace facebook
