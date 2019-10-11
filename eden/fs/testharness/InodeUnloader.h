/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <gtest/gtest.h>
#include "eden/fs/inodes/TreeInode.h"

namespace facebook {
namespace eden {

struct ConditionalUnloader {
  static size_t unload(TreeInode& unloadFrom) {
    timespec endOfTime;
    endOfTime.tv_sec = std::numeric_limits<time_t>::max();
    endOfTime.tv_nsec = 999999999;
    return unloadFrom.unloadChildrenLastAccessedBefore(endOfTime);
  }
};

struct UnconditionalUnloader {
  static size_t unload(TreeInode& unloadFrom) {
    return unloadFrom.unloadChildrenNow();
  }
};

using InodeUnloaderTypes =
    ::testing::Types<ConditionalUnloader, UnconditionalUnloader>;

} // namespace eden
} // namespace facebook
