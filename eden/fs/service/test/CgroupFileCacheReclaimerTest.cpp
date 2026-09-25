/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

#include "eden/fs/service/CgroupFileCacheReclaimer.h"

#include <unistd.h>

#include <filesystem>
#include <stdexcept>
#include <string>

#include <fmt/core.h>
#include <folly/FileUtil.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

using namespace facebook::eden;

namespace {

class CgroupFileCacheReclaimerTest : public ::testing::Test {
 protected:
  CgroupFileCacheReclaimerTest()
      : mountPoint_{(tmpDir_.path() / "cgroup2").string()},
        cgroupDirectory_{mountPoint_ + "/edenfs_test.scope"},
        procSelfCgroup_{(tmpDir_.path() / "cgroup").string()},
        procSelfMountInfo_{(tmpDir_.path() / "mountinfo").string()} {}

  void SetUp() override {
    createCgroupFiles(cgroupDirectory_);
    setProcFiles("/", mountPoint_, "/edenfs_test.scope");
  }

  void setProcFiles(
      std::string_view mountRoot,
      std::string_view mountPoint,
      std::string_view cgroupPath,
      std::string_view prefix = {}) {
    write(
        procSelfMountInfo_,
        fmt::format(
            "{}36 25 0:32 {} {} rw,nosuid,nodev - cgroup2 cgroup rw\n",
            prefix,
            mountRoot,
            mountPoint));
    write(procSelfCgroup_, fmt::format("0::{}\n", cgroupPath));
  }

  void createCgroupFiles(const std::string& directory) {
    std::filesystem::create_directories(directory);
    write(directory + "/cgroup.type", "domain\n");
    write(directory + "/cgroup.stat", "nr_descendants 0\n");
    write(directory + "/cgroup.procs", fmt::format("{}\n", getpid()));
    write(directory + "/memory.reclaim", "");
  }

  void setMemoryStat(const std::string& contents) {
    write(cgroupDirectory_ + "/memory.stat", contents);
  }

  CgroupFileCacheReclaimer makeReclaimer() const {
    return CgroupFileCacheReclaimer{procSelfCgroup_, procSelfMountInfo_};
  }

  std::string reclaimRequest() const {
    std::string contents;
    EXPECT_TRUE(
        folly::readFile(
            (cgroupDirectory_ + "/memory.reclaim").c_str(), contents));
    return contents;
  }

  static void write(const std::string& path, std::string_view contents) {
    if (!folly::writeFile(contents, path.c_str())) {
      throw std::runtime_error{fmt::format("failed to write {}", path)};
    }
  }

  folly::test::TemporaryDirectory tmpDir_;
  std::string mountPoint_;
  std::string cgroupDirectory_;
  std::string procSelfCgroup_;
  std::string procSelfMountInfo_;
};

} // namespace

TEST_F(CgroupFileCacheReclaimerTest, doesNothingBelowTarget) {
  setMemoryStat("unevictable 99\ninactive_file 300\nactive_file 400\n");
  auto reclaimer = makeReclaimer();

  const auto result = reclaimer.reclaim({
      .targetBytes = 1'000,
      .maxReclaimBytes = 500,
  });

  const CgroupFileCacheReclaimResult expected{
      .fileCacheBytesBefore = 700,
      .fileCacheBytesAfter = 700,
      .requestedBytes = 0,
  };
  EXPECT_EQ(expected, result);
  EXPECT_EQ("", reclaimRequest());
}

TEST_F(CgroupFileCacheReclaimerTest, reclaimsExcessUpToCap) {
  setMemoryStat("active_file 2000\ninactive_file 3000\nshmem 9999\n");
  auto reclaimer = makeReclaimer();

  const auto result = reclaimer.reclaim({
      .targetBytes = 1'000,
      .maxReclaimBytes = 1'500,
  });

  const CgroupFileCacheReclaimResult expected{
      .fileCacheBytesBefore = 5'000,
      .fileCacheBytesAfter = 5'000,
      .requestedBytes = 1'500,
  };
  EXPECT_EQ(expected, result);
  EXPECT_EQ("1500 swappiness=0", reclaimRequest());
}

TEST_F(CgroupFileCacheReclaimerTest, supportsFullExcessWithoutCap) {
  setMemoryStat("active_file 2000\ninactive_file 3001\n");
  auto reclaimer = makeReclaimer();

  const auto result = reclaimer.reclaim({
      .targetBytes = 1'000,
      .maxReclaimBytes = 0,
  });

  const CgroupFileCacheReclaimResult expected{
      .fileCacheBytesBefore = 5'001,
      .fileCacheBytesAfter = 5'001,
      .requestedBytes = 4'001,
  };
  EXPECT_EQ(expected, result);
  EXPECT_EQ("4001 swappiness=0", reclaimRequest());
}

TEST_F(CgroupFileCacheReclaimerTest, rejectsMalformedMemoryStat) {
  setMemoryStat("active_file 2000\n");
  auto reclaimer = makeReclaimer();

  EXPECT_THROW(
      reclaimer.reclaim({
          .targetBytes = 1'000,
          .maxReclaimBytes = 1'500,
      }),
      std::runtime_error);
}

TEST_F(CgroupFileCacheReclaimerTest, rejectsCgroupNotNamedForEdenFs) {
  setProcFiles("/", mountPoint_, "/session-1.scope");
  setMemoryStat("active_file 2000\ninactive_file 3000\n");
  auto reclaimer = makeReclaimer();

  EXPECT_THROW(
      reclaimer.reclaim({.targetBytes = 1'000, .maxReclaimBytes = 1'500}),
      NotEdenFsCgroupError);
  EXPECT_EQ("", reclaimRequest());
}

TEST_F(CgroupFileCacheReclaimerTest, reportsMissingReclaimFileAsUnsupported) {
  setMemoryStat("active_file 2000\ninactive_file 3000\n");
  std::filesystem::remove(cgroupDirectory_ + "/memory.reclaim");
  auto reclaimer = makeReclaimer();

  EXPECT_THROW(
      reclaimer.reclaim({.targetBytes = 1'000, .maxReclaimBytes = 1'500}),
      UnsupportedKernelError);
}

TEST_F(CgroupFileCacheReclaimerTest, doesNotOpenReclaimFileBelowTarget) {
  setMemoryStat("inactive_file 300\nactive_file 400\n");
  std::filesystem::remove(cgroupDirectory_ + "/memory.reclaim");
  auto reclaimer = makeReclaimer();

  const auto result = reclaimer.reclaim({
      .targetBytes = 1'000,
      .maxReclaimBytes = 500,
  });

  const CgroupFileCacheReclaimResult expected{
      .fileCacheBytesBefore = 700,
      .fileCacheBytesAfter = 700,
      .requestedBytes = 0,
  };
  EXPECT_EQ(expected, result);
}

TEST_F(CgroupFileCacheReclaimerTest, resolvesCgroupBelowMatchingMountRoot) {
  const auto ignoredMountPoint = (tmpDir_.path() / "ignored").string();
  const auto ignoredMount = fmt::format(
      "35 25 0:31 /other {} rw - cgroup2 cgroup rw\n", ignoredMountPoint);
  setProcFiles(
      "/delegated", mountPoint_, "/delegated/edenfs_test.scope", ignoredMount);
  setMemoryStat("active_file 2000\ninactive_file 3000\n");
  auto reclaimer = makeReclaimer();

  const auto result = reclaimer.reclaim({
      .targetBytes = 1'000,
      .maxReclaimBytes = 1'500,
  });

  const CgroupFileCacheReclaimResult expected{
      .fileCacheBytesBefore = 5'000,
      .fileCacheBytesAfter = 5'000,
      .requestedBytes = 1'500,
  };
  EXPECT_EQ(expected, result);
  EXPECT_EQ("1500 swappiness=0", reclaimRequest());
}

TEST_F(CgroupFileCacheReclaimerTest, resolvesCgroupBelowRootMountPoint) {
  const auto directory =
      (tmpDir_.path() / "edenfs_root_mounted.scope").string();
  createCgroupFiles(directory);
  write(directory + "/memory.stat", "active_file 2000\ninactive_file 3000\n");
  setProcFiles("/", "/", directory);
  auto reclaimer = makeReclaimer();

  const auto result = reclaimer.reclaim({
      .targetBytes = 1'000,
      .maxReclaimBytes = 1'500,
  });

  const CgroupFileCacheReclaimResult expected{
      .fileCacheBytesBefore = 5'000,
      .fileCacheBytesAfter = 5'000,
      .requestedBytes = 1'500,
  };
  EXPECT_EQ(expected, result);
  std::string request;
  ASSERT_TRUE(
      folly::readFile((directory + "/memory.reclaim").c_str(), request));
  EXPECT_EQ("1500 swappiness=0", request);
}

TEST_F(CgroupFileCacheReclaimerTest, readsMountInfoBeyond64KiB) {
  const auto prefix = fmt::format(
      "35 25 0:31 / /ignored rw {} - tmpfs tmpfs rw\n",
      std::string(70 * 1024, 'x'));
  setProcFiles("/", mountPoint_, "/edenfs_test.scope", prefix);
  setMemoryStat("active_file 2000\ninactive_file 3000\n");
  auto reclaimer = makeReclaimer();

  const auto result = reclaimer.reclaim({
      .targetBytes = 1'000,
      .maxReclaimBytes = 1'500,
  });

  const CgroupFileCacheReclaimResult expected{
      .fileCacheBytesBefore = 5'000,
      .fileCacheBytesAfter = 5'000,
      .requestedBytes = 1'500,
  };
  EXPECT_EQ(expected, result);
  EXPECT_EQ("1500 swappiness=0", reclaimRequest());
}

#endif // __linux__
