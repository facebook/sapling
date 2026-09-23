/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

#include "eden/fs/privhelper/priority/LinuxMemoryPriority.h"

#include <fcntl.h>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>
#include <sys/stat.h>
#include <unistd.h>
#include <system_error>

#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/Throw.h"
#include "eden/fs/privhelper/PrivHelperRollback.h"

namespace facebook::eden {
LinuxMemoryPriority::LinuxMemoryPriority(int32_t oomScoreAdj, uid_t expectedUid)
    : MemoryPriority(oomScoreAdj), expectedUid_(expectedUid) {
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
  const auto procPath = fmt::format("/proc/{}", pid);
  folly::File procDir(
      procPath.c_str(), O_PATH | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
  struct stat st{};
  folly::checkUnixError(fstat(procDir.fd(), &st), "stat ", procPath);
  // The daemon also configures the root-owned privhelper's own priority.
  if (pid != getpid() && st.st_uid != expectedUid_) {
    if (!disablePrivHelperHardening()) {
      folly::throwSystemErrorExplicit(
          EPERM,
          fmt::format("process {} is not owned by user {}", pid, expectedUid_));
    }
    XLOGF(
        WARN,
        "Skipping ownership check for process {} because privhelper hardening is disabled",
        pid);
  }

  const auto fd =
      openat(procDir.fd(), "oom_score_adj", O_WRONLY | O_CLOEXEC | O_NOFOLLOW);
  folly::checkUnixError(fd, "open oom_score_adj for process ", pid);
  folly::File output(fd, true);
  auto oomScoreAdj = std::to_string(priority_);
  if (folly::writeFull(output.fd(), oomScoreAdj.data(), oomScoreAdj.size()) !=
      static_cast<ssize_t>(oomScoreAdj.size())) {
    XLOGF(
        ERR,
        "Failed to set oom_score_adj for process {}: {}",
        pid,
        std::system_category().message(errno));
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
