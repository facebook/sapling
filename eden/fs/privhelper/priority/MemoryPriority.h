/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sys/types.h>
#include <optional>

namespace facebook::eden {

/*
 * MemoryPriority allows us to specify and set memory priorities (i.e. JetSam
 * priority on macOS, /proc/<pid>/oom_score_adj on Linux) for a given process.
 */
class MemoryPriority {
 public:
  explicit MemoryPriority(int32_t priority) : priority_(priority) {}
  virtual ~MemoryPriority() = default;

  // Sets the memory priority for a given process to the value supplied at
  // construction. Returns 0 on success, and -1 on failure.
  virtual int setPriorityForProcess(pid_t pid) = 0;

  // Returns the target memory priority that was supplied at construction. This
  // value will be used for subsequent calls to setPriorityForProcess.
  int32_t getTargetPriority() {
    return priority_;
  }

  // Returns the actual memory priority for the given process. This is fetched
  // from the appropriate source (e.g. /proc/<pid>/oom_score_adj on Linux, or
  // memstatus_control on macOS).
  virtual std::optional<int32_t> getPriorityForProcess(pid_t pid) = 0;

  // TODO: Add support for querying the current memory priority of a process

 protected:
  int32_t priority_;
};

} // namespace facebook::eden
