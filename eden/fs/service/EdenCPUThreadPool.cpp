/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
