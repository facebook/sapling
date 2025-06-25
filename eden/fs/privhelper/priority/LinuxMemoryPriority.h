/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/privhelper/priority/MemoryPriority.h"

namespace facebook::eden {

// Contains platform-specific logic for setting memory priorities on Linux.
//
// This sets the value inside /proc/<pid>/oom_score_adj which is used to
// determine the order that the OOM killer should kill processes with heavy
// memory consumption. Source:
// https://unix.stackexchange.com/questions/153585/how-does-the-oom-killer-decide-which-process-to-kill-first
//
// TODO: this may need to be distro specific in the future
class LinuxMemoryPriority : public MemoryPriority {
 public:
  explicit LinuxMemoryPriority(int32_t oomScoreAdj);
  ~LinuxMemoryPriority() override = default;

  int setPriorityForProcess(pid_t pid) override;
  std::optional<int32_t> getPriorityForProcess(pid_t pid) override;
};

} // namespace facebook::eden
