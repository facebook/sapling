/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#if defined(__linux__) || defined(__APPLE__)

#include "eden/fs/privhelper/PinScan.h"

#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <filesystem>
#include <fstream>
#include <set>
#include <utility>

#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

#ifdef __APPLE__
#include <libproc.h> // @manual
#include <sys/mman.h>
#include <sys/proc_info.h> // @manual
#include <sys/wait.h>
#endif

using namespace facebook::eden;

namespace {

struct stat statPath(const std::filesystem::path& path) {
  struct stat st{};
  EXPECT_EQ(0, ::stat(path.c_str(), &st));
  return st;
}

} // namespace

#ifdef __linux__

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

#ifdef __APPLE__

extern "C" int pthread_chdir_np(const char*);

namespace {

enum class PinQuery { None, Directories, ThreadDirectory, File };

struct PinQueryFailure {
  const char* name;
  PinQuery query;
  int error;
  // Whether the error fails the scan rather than skipping the reference.
  bool failsScan;
};

class ProcessPinScanTest : public ::testing::TestWithParam<PinQueryFailure> {};

} // namespace

TEST_P(ProcessPinScanTest, scanProcessPins) {
  folly::test::TemporaryDirectory tmpDir;
  auto root = std::filesystem::path{tmpDir.path().string()};
  auto cwd = root / "cwd";
  auto threadCwd = root / "thread-cwd";
  std::filesystem::create_directories(cwd);
  std::filesystem::create_directories(threadCwd);
  auto openFile = root / "open.txt";
  auto mappedFile = root / "mapped.txt";
  std::ofstream{openFile} << "open";
  std::ofstream{mappedFile} << "mapped";

  // A child process pins the directory as its cwd, holds one file open and
  // has the other mapped. It reports over `ready` once it has, then waits on
  // `done` so the pins outlive the scan.
  int ready[2];
  int done[2];
  ASSERT_EQ(0, pipe(ready));
  ASSERT_EQ(0, pipe(done));
  pid_t child = fork();
  ASSERT_GE(child, 0);
  if (child == 0) {
    close(ready[0]);
    close(done[1]);
    if (chdir(cwd.c_str()) != 0) {
      _exit(1);
    }
    if (pthread_chdir_np(threadCwd.c_str()) != 0) {
      _exit(4);
    }
    if (open(openFile.c_str(), O_RDONLY) < 0) {
      _exit(2);
    }
    int mappedFd = open(mappedFile.c_str(), O_RDONLY);
    if (mappedFd < 0 ||
        mmap(nullptr, 1, PROT_READ, MAP_PRIVATE, mappedFd, 0) == MAP_FAILED) {
      _exit(3);
    }
    close(mappedFd);
    char byte = 1;
    (void)write(ready[1], &byte, 1);
    (void)read(done[0], &byte, 1);
    _exit(0);
  }
  close(ready[1]);
  close(done[0]);
  char byte = 0;
  ASSERT_EQ(1, read(ready[0], &byte, 1));

  // Widened the way the scanner widens what libproc reports.
  auto dev =
      static_cast<uint64_t>(static_cast<uint32_t>(statPath(root).st_dev));
  auto openFileIno = statPath(openFile).st_ino;
  auto failure = GetParam();
  auto pidInfo = [&](int pid, int flavor, uint64_t arg, void* data, int size) {
    if (pid == child &&
        ((failure.query == PinQuery::Directories &&
          flavor == PROC_PIDVNODEPATHINFO) ||
         (failure.query == PinQuery::ThreadDirectory &&
          flavor == PROC_PIDTHREADPATHINFO))) {
      errno = failure.error;
      return 0;
    }
    return proc_pidinfo(pid, flavor, arg, data, size);
  };
  auto pidFdInfo = [&](int pid, int fd, int flavor, void* data, int size) {
    auto result = proc_pidfdinfo(pid, fd, flavor, data, size);
    if (pid == child && failure.query == PinQuery::File &&
        flavor == PROC_PIDFDVNODEPATHINFO && result == size &&
        static_cast<vnode_fdinfowithpath*>(data)->pvip.vip_vi.vi_stat.vst_ino ==
            openFileIno) {
      errno = failure.error;
      return 0;
    }
    return result;
  };
  auto pins = scanProcessPins({dev}, pidInfo, pidFdInfo);

  byte = 0;
  (void)write(done[1], &byte, 1);
  close(done[1]);
  int status = 0;
  ASSERT_EQ(child, waitpid(child, &status, 0));
  EXPECT_TRUE(WIFEXITED(status) && WEXITSTATUS(status) == 0);

  if (failure.failsScan) {
    ASSERT_TRUE(pins.hasError());
    EXPECT_EQ(failure.error, pins.error());
    return;
  }
  ASSERT_TRUE(pins.hasValue());
  std::set<PinnedInode> found{pins->begin(), pins->end()};
  auto pinOf = [&](const std::filesystem::path& path) {
    return PinnedInode{dev, static_cast<uint64_t>(statPath(path).st_ino)};
  };
  EXPECT_EQ(1u, found.count(pinOf(mappedFile)));
  // The reference whose query failed is skipped and the rest are still found.
  const std::pair<PinQuery, std::filesystem::path> pathsByQuery[] = {
      {PinQuery::Directories, cwd},
      {PinQuery::ThreadDirectory, threadCwd},
      {PinQuery::File, openFile},
  };
  for (const auto& [query, path] : pathsByQuery) {
    EXPECT_EQ(query == failure.query ? 0u : 1u, found.count(pinOf(path)))
        << path;
  }
}

INSTANTIATE_TEST_SUITE_P(
    VnodeQueries,
    ProcessPinScanTest,
    ::testing::Values(
        PinQueryFailure{"Success", PinQuery::None, 0, false},
        PinQueryFailure{"MissingCwd", PinQuery::Directories, ENOENT, false},
        PinQueryFailure{
            "StaleThreadCwd",
            PinQuery::ThreadDirectory,
            ESTALE,
            false},
        PinQueryFailure{"MissingFile", PinQuery::File, ENOENT, false},
        PinQueryFailure{"IoError", PinQuery::Directories, EIO, true}),
    [](const auto& info) { return info.param.name; });

TEST(PinScanTest, scanProcessPinsWithoutMatchingDevices) {
  // Devices are widened from 32 bits, so this one matches no vnode.
  auto nothing = scanProcessPins({uint64_t{1} << 40});
  ASSERT_TRUE(nothing.hasValue());
  EXPECT_TRUE(nothing->empty());
  EXPECT_TRUE(scanProcessPins({}).value().empty());
}

TEST(PinScanTest, listMountsForPinScanIncludesRoot) {
  auto mounts = listMountsForPinScan();
  ASSERT_TRUE(mounts.hasValue());
  auto rootMount = std::find_if(
      mounts->begin(), mounts->end(), [](const PinScanMount& mount) {
        return mount.mountPoint == "/";
      });
  ASSERT_NE(mounts->end(), rootMount);
  EXPECT_FALSE(rootMount->fsType.empty());
  EXPECT_NE(0u, rootMount->dev);
}

#endif // __APPLE__

TEST(PinScanTest, reportRoundTrip) {
  PinScanReport report;
  report.scannedDevices = {7, 42};
  report.pinsByDevice[42] = {1, 6654235};

  auto parsed = parsePinScanReport(formatPinScanReport(report));
  ASSERT_TRUE(parsed.has_value());
  EXPECT_EQ(report.scannedDevices, parsed->scannedDevices);
  EXPECT_EQ(report.pinsByDevice, parsed->pinsByDevice);
}

TEST(PinScanTest, parseRejectsIncompleteOrMalformedReports) {
  EXPECT_FALSE(parsePinScanReport(""));
  EXPECT_FALSE(parsePinScanReport("dev 7\n42 1\n"));
  EXPECT_FALSE(parsePinScanReport("dev 7\n42 one\ndone\n"));
  EXPECT_FALSE(parsePinScanReport("dev\ndone\n"));

  // A scanner that predates device lines reports pins only. Nothing counts
  // as covered, so consumers treat every mount's pins as unknown.
  auto legacy = parsePinScanReport("42 1\ndone\n");
  ASSERT_TRUE(legacy.has_value());
  EXPECT_TRUE(legacy->scannedDevices.empty());
  EXPECT_EQ(1u, legacy->pinsByDevice.count(42));
}

#endif // __linux__ || __APPLE__
