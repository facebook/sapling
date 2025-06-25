/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

#include "eden/fs/privhelper/priority/LinuxMemoryPriority.h"

#include <folly/logging/xlog.h>

#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/Throw.h"

namespace facebook::eden {
LinuxMemoryPriority::LinuxMemoryPriority(int32_t oomScoreAdj)
    : MemoryPriority(oomScoreAdj) {
  // oom_score_adj ranges from -1000 to 1000, with 1000 being the most likely to
  // be killed, and -1000 being very unlikely to be killed.
  if (oomScoreAdj < -1000 || oomScoreAdj > 1000) {
    throwf<std::invalid_argument>(
        "Invalid oomScoreAdj: {}. Value must be between -1000 and 1000 inclusive.",
        oomScoreAdj);
  }

  // The current default oomScoreAdj is 0, which means setting a priority
  // higher will make EdenFS more likely to be killed.
  if (oomScoreAdj > 0) {
    XLOGF(
        WARN,
        "Setting oomScoreAdj above 0 is not recommended. Priority: {}",
        oomScoreAdj);
  }
}

int LinuxMemoryPriority::setPriorityForProcess(pid_t pid) {
  auto oomScoreAdjPath =
      canonicalPath({fmt::format("/proc/{}/oom_score_adj", pid)});
  auto oomScoreAdj = std::to_string(priority_);
  auto writeResult = writeFile(oomScoreAdjPath, folly::ByteRange{oomScoreAdj});
  if (writeResult.hasException()) {
    XLOGF(
        ERR,
        "Failed to set oom_score_adj for process {}: {}",
        pid,
        writeResult.exception().what());
    return -1;
  }
  XLOGF(INFO, "The priority of {} was set to {} successfully.", pid, priority_);
  return 0;
}

std::optional<int32_t> LinuxMemoryPriority::getPriorityForProcess(pid_t pid) {
  auto oomScoreAdjPath =
      canonicalPath({fmt::format("/proc/{}/oom_score_adj", pid)});
  auto readResult = readFile(oomScoreAdjPath);
  if (readResult.hasException()) {
    XLOGF(
        ERR,
        "Failed to read oom_score_adj for process {}: {}",
        pid,
        readResult.exception().what());
    return std::nullopt;
  } else {
    try {
      auto oomScoreAdj = folly::to<int32_t>(readResult.value());
      return oomScoreAdj;
    } catch (const std::exception& e) {
      XLOGF(ERR, "Failed to parse oom_score_adj as an int: {}", e.what());
      return std::nullopt;
    }
  }
}
} // namespace facebook::eden

#endif // __linux__
