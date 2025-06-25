/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Portability.h>
#include <folly/portability/SysTypes.h>
#include <sys/types.h>
#include <memory>
#include <optional>

#include "eden/fs/privhelper/priority/MemoryPriority.h"

namespace facebook::eden {

// TODO: Implement a WindowsMemoryPriority if memory consumption becomes an
// issue on Windows

class ProcessPriority {
 public:
  explicit ProcessPriority(std::optional<int32_t> memoryPriority);

  // TODO: Add other priority types (ex. nice value)

  int setPrioritiesForProcess(pid_t pid);

 private:
  // The kernel can respond to memory pressure situations in many ways,
  // including killing processes with heavy memory usage. EdenFS is often caught
  // in the crossfire during these events, since EdenFS relies on large amounts
  // of file-backed memory for Sapling caches. The memory priority value is
  // intended to hint to the kernel that it should avoid killing EdenFS if
  // possible.
  std::optional<std::shared_ptr<MemoryPriority>> memoryPriority_;
};

} // namespace facebook::eden
