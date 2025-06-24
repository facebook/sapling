/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/privhelper/priority/ProcessPriority.h"

#include <folly/logging/xlog.h>

#ifdef __linux__
#include "eden/fs/privhelper/priority/LinuxMemoryPriority.h"
#elif defined(__APPLE__) // __linux__
#include "eden/fs/privhelper/priority/DarwinMemoryPriority.h"
#endif // __APPLE__

namespace facebook::eden {

ProcessPriority::ProcessPriority(std::optional<int> memoryPriority) {
  if (memoryPriority.has_value()) {
#ifdef __linux__
    memoryPriority_ = std::make_shared<LinuxMemoryPriority>(
        /*oomScoreAdj=*/memoryPriority.value());
#elif defined(__APPLE__) // __linux__
    memoryPriority_ = std::make_shared<DarwinMemoryPriority>(
        /*jetsamPriority=*/memoryPriority.value());
#else // __APPLE__
    XLOGF(
        ERR,
        "Unsupported platform for MemoryPriority. Only Linux and macOS are supported.");
    memoryPriority_ = std::nullopt;
#endif // !__APPLE__ && !__linux__
  } else {
    memoryPriority_ = std::nullopt;
  }
}

int ProcessPriority::setPrioritiesForProcess(pid_t pid) {
  if (memoryPriority_.has_value()) {
    if (memoryPriority_.value()->setPriorityForProcess(pid)) {
      XLOGF(
          ERR,
          "Failed to set memory priority for process {} to {}",
          pid,
          memoryPriority_.value()->getTargetPriority());
    }
  }
  return 0;
}

} // namespace facebook::eden
