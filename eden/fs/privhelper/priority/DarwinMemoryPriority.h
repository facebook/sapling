/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/privhelper/priority/MemoryPriority.h"

namespace facebook::eden {

// Contains platform-specific logic for setting memory priorities on macOS.
//
// This currently uses an undocumented macOS API to directly set the Jetsam
// priority of a given process. This is not a public API and is subject to
// change without notice, and therefore any failed attempts to set the priority
// will be ignored. Sources:
// https://www.newosxbook.com/articles/MemoryPressure.html
//
// Implementation based on: https://github.com/asdfugil/overb0ard
class DarwinMemoryPriority : public MemoryPriority {
 public:
  explicit DarwinMemoryPriority(int32_t jetsam_priority);
  ~DarwinMemoryPriority() override = default;

  int setPriorityForProcess(pid_t pid) override;
  std::optional<int> getPriorityForProcess(pid_t pid) override;
};

} // namespace facebook::eden
