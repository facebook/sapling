/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ProcessNameCache.h"
#include <gtest/gtest.h>

using namespace std::literals;
using namespace facebook::eden;

TEST(ProcessNameCache, getProcPidCmdLine) {
  using namespace facebook::eden::detail;
  EXPECT_EQ("/proc/0/cmdline"s, getProcPidCmdLine(0).data());
  EXPECT_EQ("/proc/1234/cmdline"s, getProcPidCmdLine(1234).data());
  EXPECT_EQ("/proc/1234/cmdline"s, getProcPidCmdLine(1234).data());

  auto longestPath = getProcPidCmdLine(std::numeric_limits<pid_t>::max());
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

TEST(ProcessNameCache, addFromMultipleThreads) {
  ProcessNameCache processNameCache;

  size_t kThreadCount = 32;
  std::vector<std::thread> threads;
  threads.reserve(kThreadCount);
  for (size_t i = 0; i < kThreadCount; ++i) {
    threads.emplace_back([&] { processNameCache.add(getpid()); });
  }

  auto results = processNameCache.getAllProcessNames();

  for (auto& thread : threads) {
    thread.join();
  }
  EXPECT_EQ(1, results.size());
}
