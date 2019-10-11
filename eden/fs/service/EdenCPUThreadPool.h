/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/utils/UnboundedQueueExecutor.h"

namespace facebook {
namespace eden {

// The Eden CPU thread pool is intended for miscellaneous background tasks.
class EdenCPUThreadPool : public UnboundedQueueExecutor {
 public:
  explicit EdenCPUThreadPool();
};

} // namespace eden
} // namespace facebook
