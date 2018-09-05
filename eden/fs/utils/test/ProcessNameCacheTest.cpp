/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/ProcessNameCache.h"
#include <gtest/gtest.h>

using namespace std::literals;
using namespace facebook::eden;

TEST(ProcessNameCache, getProcPidExe) {
  using namespace facebook::eden::detail;
  EXPECT_EQ("/proc/0/exe"s, getProcPidExe(0).data());
  EXPECT_EQ("/proc/1234/exe"s, getProcPidExe(1234).data());
  EXPECT_EQ("/proc/1234/exe"s, getProcPidExe(1234).data());

  auto longestPath = getProcPidExe(std::numeric_limits<pid_t>::max());
  EXPECT_EQ(longestPath.size(), strlen(longestPath.data()) + 1);
}

TEST(ProcessNameCache, readMyPidsName) {
  ProcessNameCache processNameCache;
  processNameCache.add(getpid());
  auto results = processNameCache.getAllProcessNames();
  EXPECT_NE("", results[getpid()]);
}

TEST(ProcessNameCache, expireMyPidsName) {
  ProcessNameCache processNameCache{0ms};
  processNameCache.add(getpid());
  auto results = processNameCache.getAllProcessNames();
  EXPECT_EQ(0, results.size());
}
