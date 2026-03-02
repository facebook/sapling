/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/futures/Future.h>
#include <folly/io/async/EventBaseThread.h>
#include <folly/synchronization/CallOnce.h>
#include <filesystem>
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/benchmarks/bench_utils.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"

DEFINE_string(repo, "", "Repository to run the benchmark against");
DEFINE_string(
    file_paths,
    "",
    "Comma-separated list of file paths to fetch (relative to repo root)");
DEFINE_string(
    directory,
    "",
    "Directory to recursively discover files from (relative to repo root, defaults to fbcode/eden/scm/tests)");

namespace {

using namespace facebook::eden;
using namespace facebook::eden::benchmarks;

std::vector<std::string> getFilePathsFromDirectory(
    const AbsolutePath& repoPath,
    const std::string& directory) {
  std::vector<std::string> filePaths;

  std::filesystem::path dirPath = repoPath.asString();
  dirPath /= directory;

  if (!std::filesystem::exists(dirPath)) {
    throw std::invalid_argument(
        "Directory does not exist: " + dirPath.string());
  }

  if (!std::filesystem::is_directory(dirPath)) {
    throw std::invalid_argument("Path is not a directory: " + dirPath.string());
  }

  for (const auto& entry :
       std::filesystem::recursive_directory_iterator(dirPath)) {
    if (entry.is_regular_file()) {
      auto relativePath =
          std::filesystem::relative(entry.path(), repoPath.asString());
      filePaths.push_back(relativePath.string());
    }
  }

  if (filePaths.empty()) {
    throw std::invalid_argument("No files found in directory: " + directory);
  }

  return filePaths;
}

std::vector<std::string> parseFilePaths(const AbsolutePath& repoPath) {
  std::vector<std::string> filePaths;

  if (!FLAGS_directory.empty()) {
    filePaths = getFilePathsFromDirectory(repoPath, FLAGS_directory);
  } else if (!FLAGS_file_paths.empty()) {
    std::string paths = FLAGS_file_paths;
    size_t start = 0;
    size_t end = 0;

    while ((end = paths.find(',', start)) != std::string::npos) {
      std::string path = paths.substr(start, end - start);
      path.erase(0, path.find_first_not_of(" \t\r\n"));
      path.erase(path.find_last_not_of(" \t\r\n") + 1);
      if (!path.empty()) {
        filePaths.push_back(path);
      }
      start = end + 1;
    }

    std::string path = paths.substr(start);
    path.erase(0, path.find_first_not_of(" \t\r\n"));
    path.erase(path.find_last_not_of(" \t\r\n") + 1);
    if (!path.empty()) {
      filePaths.push_back(path);
    }
  } else {
    filePaths = getFilePathsFromDirectory(repoPath, "fbcode/eden/scm/tests");
  }

  if (filePaths.empty()) {
    throw std::invalid_argument(
        "No file paths provided. Use --file_paths, --directory, or let it use the default directory (fbcode/eden/scm/tests)");
  }

  return filePaths;
}

AbsolutePath validateArguments() {
  if (FLAGS_repo.empty()) {
    throw std::invalid_argument("A repo must be passed in");
  }

  return canonicalPath(FLAGS_repo);
}

void eden_get_file_content(benchmark::State& state) {
  auto path = validateArguments();
  auto filePaths = parseFilePaths(path);
  if (filePaths.empty()) {
    throw std::invalid_argument("No file paths provided");
  }

  static folly::once_flag printOnce;
  folly::call_once(printOnce, [&filePaths] {
    fprintf(stderr, "Found %zu files to benchmark\n", filePaths.size());
  });

  auto socketPath = getEdenSocketPath(path);

  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  auto client = createEdenThriftClient(eventBase, socketPath);

  std::vector<GetFileContentRequest> requests;
  requests.reserve(filePaths.size());

  for (const auto& filePath : filePaths) {
    GetFileContentRequest request;
    request.mount() = MountId();
    request.mount()->mountPoint() = path.view();
    request.filePath() = filePath;
    request.sync() = SyncBehavior();
    requests.push_back(std::move(request));
  }

  size_t requestIndex = 0;
  size_t totalErrors = 0;

  for (auto _ : state) {
    auto totalTime = std::chrono::duration<double>::zero();
    for (auto& request : requests) {
      requestIndex++;

      auto start = std::chrono::high_resolution_clock::now();
      auto result =
          client->semifuture_getFileContent(request).via(eventBase).get();
      auto end = std::chrono::high_resolution_clock::now();

      benchmark::DoNotOptimize(result);

      bool hasError = true;
      if (apache::thrift::is_non_optional_field_set_manually_or_by_serializer(
              result.blob())) {
        const auto& blobOrError = *result.blob();
        if (blobOrError.getType() == ScmBlobOrError::Type::blob) {
          const auto& blobData = blobOrError.get_blob();
          if (!blobData.empty()) {
            hasError = false;
          }
        }
      }

      if (hasError) {
        totalErrors++;
      }

      auto elapsed = std::chrono::duration_cast<std::chrono::duration<double>>(
          end - start);
      totalTime += elapsed;
    }
    state.SetIterationTime(totalTime.count());
  }

  state.counters["total_requests"] = requestIndex;
  state.counters["total_errors"] = totalErrors;
}

void eden_get_file_content_concurrent(benchmark::State& state) {
  auto path = validateArguments();
  auto filePaths = parseFilePaths(path);
  if (filePaths.empty()) {
    throw std::invalid_argument("No file paths provided");
  }

  static folly::once_flag printOnce;
  folly::call_once(printOnce, [&filePaths] {
    fprintf(
        stderr,
        "Found %zu files to benchmark (concurrent)\n",
        filePaths.size());
  });

  auto socketPath = getEdenSocketPath(path);

  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  auto client = createEdenThriftClient(eventBase, socketPath);

  std::vector<GetFileContentRequest> requests;
  requests.reserve(filePaths.size());

  for (const auto& filePath : filePaths) {
    GetFileContentRequest request;
    request.mount() = MountId();
    request.mount()->mountPoint() = path.view();
    request.filePath() = filePath;
    request.sync() = SyncBehavior();
    requests.push_back(std::move(request));
  }

  size_t totalRequests = 0;
  size_t totalErrors = 0;

  for (auto _ : state) {
    // Fire all requests concurrently
    std::vector<folly::SemiFuture<GetFileContentResponse>> futures;
    futures.reserve(requests.size());

    auto start = std::chrono::high_resolution_clock::now();

    for (auto& request : requests) {
      futures.push_back(client->semifuture_getFileContent(request));
    }

    // Wait for all to complete
    auto results = folly::collectAll(std::move(futures)).via(eventBase).get();

    auto end = std::chrono::high_resolution_clock::now();

    for (auto& resultTry : results) {
      totalRequests++;
      bool hasError = true;
      if (resultTry.hasValue()) {
        auto& result = resultTry.value();
        benchmark::DoNotOptimize(result);
        if (apache::thrift::is_non_optional_field_set_manually_or_by_serializer(
                result.blob())) {
          const auto& blobOrError = *result.blob();
          if (blobOrError.getType() == ScmBlobOrError::Type::blob) {
            const auto& blobData = blobOrError.get_blob();
            if (!blobData.empty()) {
              hasError = false;
            }
          }
        }
      }
      if (hasError) {
        totalErrors++;
      }
    }

    auto elapsed =
        std::chrono::duration_cast<std::chrono::duration<double>>(end - start);
    state.SetIterationTime(elapsed.count());
  }

  state.counters["total_requests"] = totalRequests;
  state.counters["total_errors"] = totalErrors;
}

BENCHMARK(eden_get_file_content)
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond)
    ->Repetitions(10);

BENCHMARK(eden_get_file_content_concurrent)
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond)
    ->Repetitions(10);

} // namespace

EDEN_BENCHMARK_MAIN();
