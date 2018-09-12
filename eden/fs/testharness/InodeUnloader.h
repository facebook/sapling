/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
