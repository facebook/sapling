/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>

#include <folly/Range.h>

namespace facebook::eden {

struct LinuxKernelVersion {
  uint32_t major{};
  uint32_t minor{};
};

LinuxKernelVersion parseLinuxKernelVersion(folly::StringPiece release);

LinuxKernelVersion getRunningLinuxKernelVersion();

} // namespace facebook::eden
