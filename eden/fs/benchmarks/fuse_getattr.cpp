/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <fcntl.h>
#include <sys/stat.h>

#include <folly/Exception.h>
#include <gflags/gflags.h>

#include <cstddef>
#include <filesystem>
#include <stdexcept>
#include <string>
#include <vector>

#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/benchharness/Bench.h"

DEFINE_string(repo, "", "EdenFS checkout to run the benchmark against");
DEFINE_string(
    file_paths,
    "",
    "Comma-separated paths to query, relative to the checkout root");
DEFINE_string(
    directory,
    "",
    "Directory whose regular files should be queried recursively, relative "
    "to the checkout root");

namespace {

using namespace facebook::eden;

AbsolutePath validateRepo() {
  if (FLAGS_repo.empty()) {
    throw std::invalid_argument("An EdenFS checkout must be passed in --repo");
  }
  return canonicalPath(FLAGS_repo);
}

/** Parse the comma-separated file list before measurement begins. */
std::vector<AbsolutePath> parseFilePaths(const AbsolutePath& repo) {
  std::vector<AbsolutePath> paths;
  size_t start = 0;
  while (start < FLAGS_file_paths.size()) {
    auto end = FLAGS_file_paths.find(',', start);
    auto value = FLAGS_file_paths.substr(
        start, end == std::string::npos ? std::string::npos : end - start);
    auto first = value.find_first_not_of(" \t\r\n");
    if (first != std::string::npos) {
      auto last = value.find_last_not_of(" \t\r\n");
      paths.push_back(
          repo + RelativePath{value.substr(first, last - first + 1)});
    }
    if (end == std::string::npos) {
      break;
    }
    start = end + 1;
  }

  if (paths.empty()) {
    throw std::invalid_argument("No valid file paths were provided");
  }
  return paths;
}

/** Recursively discover regular files before measurement begins. */
std::vector<AbsolutePath> discoverDirectoryPaths(const AbsolutePath& repo) {
  auto directory = repo + RelativePath{FLAGS_directory};
  if (!std::filesystem::is_directory(directory.asString())) {
    throw std::invalid_argument(
        "Directory does not exist: " + directory.asString());
  }

  std::vector<AbsolutePath> paths;
  for (const auto& entry :
       std::filesystem::recursive_directory_iterator(directory.asString())) {
    if (entry.is_regular_file()) {
      paths.push_back(canonicalPath(entry.path().string()));
    }
  }
  if (paths.empty()) {
    throw std::invalid_argument(
        "Directory contains no regular files: " + directory.asString());
  }
  return paths;
}

/** Select exactly one caller-provided path source. */
std::vector<AbsolutePath> getPaths(const AbsolutePath& repo) {
  if (FLAGS_file_paths.empty() == FLAGS_directory.empty()) {
    throw std::invalid_argument(
        "Pass exactly one of --file_paths or --directory");
  }
  return FLAGS_directory.empty() ? parseFilePaths(repo)
                                 : discoverDirectoryPaths(repo);
}

/** Bypass the kernel attribute cache so each call reaches EdenFS. */
struct statx forceGetattr(const AbsolutePath& path) {
  struct statx result{};
  folly::checkUnixError(
      ::statx(
          AT_FDCWD,
          path.c_str(),
          AT_SYMLINK_NOFOLLOW | AT_STATX_FORCE_SYNC,
          STATX_BASIC_STATS,
          &result),
      "statx failed");
  return result;
}

void edenFuseGetattr(benchmark::State& state) {
  auto paths = getPaths(validateRepo());
  for (const auto& path : paths) {
    benchmark::DoNotOptimize(forceGetattr(path).stx_ino);
  }

  for (auto _ : state) {
    for (const auto& path : paths) {
      benchmark::DoNotOptimize(forceGetattr(path).stx_ino);
    }
  }

  state.SetItemsProcessed(state.iterations() * paths.size());
  state.counters["paths"] = static_cast<double>(paths.size());
}

BENCHMARK(edenFuseGetattr)->Unit(benchmark::kMillisecond);

} // namespace

EDEN_BENCHMARK_MAIN();
