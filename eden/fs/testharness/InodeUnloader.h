/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/portability/GTest.h>
#include "eden/fs/inodes/TreeInode.h"

namespace facebook::eden {

#ifndef _WIN32
struct ConditionalUnloader {
  static size_t unload(TreeInode& unloadFrom) {
    timespec endOfTime;
    endOfTime.tv_sec = std::numeric_limits<time_t>::max();
    endOfTime.tv_nsec = 999999999;
    return unloadFrom.unloadChildrenLastAccessedBefore(endOfTime);
  }
};
#endif

struct UnconditionalUnloader {
  static size_t unload(TreeInode& unloadFrom) {
    return unloadFrom.unloadChildrenNow();
  }
};

#ifndef _WIN32
using InodeUnloaderTypes =
    ::testing::Types<ConditionalUnloader, UnconditionalUnloader>;
#else
using InodeUnloaderTypes = ::testing::Types<UnconditionalUnloader>;
#endif

} // namespace facebook::eden
