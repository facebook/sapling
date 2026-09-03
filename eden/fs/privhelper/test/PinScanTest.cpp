/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

#include "eden/fs/privhelper/PinScan.h"

#include <sys/stat.h>

#include <cerrno>
#include <filesystem>

#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(PinScanTest, parseFuseUserId) {
  EXPECT_EQ(
      373535,
      parseFuseUserId(
          "rw,user_id=373535,group_id=100,default_permissions,allow_other"));
  EXPECT_EQ(0, parseFuseUserId("user_id=0"));
  EXPECT_EQ(42, parseFuseUserId("rw,allow_other,user_id=42"));
  EXPECT_EQ(std::nullopt, parseFuseUserId(""));
  EXPECT_EQ(std::nullopt, parseFuseUserId("rw,group_id=100"));
  EXPECT_EQ(std::nullopt, parseFuseUserId("rw,user_id=bogus"));
  EXPECT_EQ(std::nullopt, parseFuseUserId("rw,user_id="));
}

namespace {

struct stat statPath(const std::filesystem::path& path) {
  struct stat st{};
  EXPECT_EQ(0, ::stat(path.c_str(), &st));
  return st;
}

} // namespace

TEST(PinScanTest, scanProcessPins) {
  folly::test::TemporaryDirectory tmpDir;
  auto root = std::filesystem::path{tmpDir.path().string()};

  // Fake proc layout: two processes with cwd/root links into "repo", one
  // process with a dangling link, and a non-numeric entry to be ignored.
  auto repo = root / "repo";
  auto subdir = repo / "sub";
  std::filesystem::create_directories(subdir);

  auto proc = root / "proc";
  std::filesystem::create_directories(proc / "123");
  std::filesystem::create_symlink(subdir, proc / "123" / "cwd");
  std::filesystem::create_symlink(repo, proc / "123" / "root");
  std::filesystem::create_directories(proc / "456");
  std::filesystem::create_symlink(subdir, proc / "456" / "cwd");
  std::filesystem::create_directories(proc / "789");
  std::filesystem::create_symlink(root / "gone", proc / "789" / "cwd");
  std::filesystem::create_directories(proc / "self");
  std::filesystem::create_symlink(repo, proc / "self" / "cwd");

  auto repoStat = statPath(repo);
  auto subdirStat = statPath(subdir);
  auto dev = static_cast<uint64_t>(repoStat.st_dev);

  auto pins = scanProcessPins({dev}, proc.c_str());
  std::vector<PinnedInode> expected{
      {dev, static_cast<uint64_t>(repoStat.st_ino)},
      {dev, static_cast<uint64_t>(subdirStat.st_ino)}};
  std::sort(expected.begin(), expected.end());
  ASSERT_TRUE(pins.hasValue());
  EXPECT_EQ(expected, pins.value());

  // A device filter matching nothing returns no pins.
  EXPECT_TRUE(scanProcessPins({dev + 12345}, proc.c_str()).value().empty());
  EXPECT_TRUE(scanProcessPins({}, proc.c_str()).value().empty());

  // An unreadable proc root is an error, not an empty result.
  auto missing = scanProcessPins({dev}, (root / "missing").c_str());
  ASSERT_TRUE(missing.hasError());
  EXPECT_EQ(ENOENT, missing.error());
}

#endif // __linux__
