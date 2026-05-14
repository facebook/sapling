/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <stdexcept>
#include <string>
#include <vector>

#include <folly/io/async/EventBaseThread.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/benchmarks/bench_utils.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"

DEFINE_string(repo, "", "Repository to run the benchmark against");
DEFINE_string(
    socket_path,
    "",
    "Optional explicit Eden thrift socket path override");
DEFINE_string(
    file_paths,
    "",
    "Comma-separated list of file paths to hash, relative to repo root");
DEFINE_int64(
    sync_timeout_seconds,
    0,
    "Value for SyncBehavior.syncTimeoutSeconds. Use 0 to skip synchronization");

namespace {

using namespace facebook::eden;
using namespace facebook::eden::benchmarks;

AbsolutePath validateArguments() {
  if (FLAGS_repo.empty()) {
    throw std::invalid_argument("A repo must be passed in");
  }
  if (FLAGS_file_paths.empty()) {
    throw std::invalid_argument(
        "A comma-separated file_paths argument is required");
  }

  return canonicalPath(FLAGS_repo);
}

std::vector<std::string> parseFilePaths() {
  std::vector<std::string> filePaths;
  std::string paths = FLAGS_file_paths;
  size_t start = 0;

  while (start < paths.size()) {
    auto end = paths.find(',', start);
    std::string path = paths.substr(
        start, end == std::string::npos ? std::string::npos : end - start);
    path.erase(0, path.find_first_not_of(" \t\r\n"));
    path.erase(path.find_last_not_of(" \t\r\n") + 1);
    if (!path.empty()) {
      filePaths.push_back(std::move(path));
    }
    if (end == std::string::npos) {
      break;
    }
    start = end + 1;
  }

  if (filePaths.empty()) {
    throw std::invalid_argument("No valid file paths were provided");
  }

  return filePaths;
}

void eden_get_sha1(benchmark::State& state) {
  auto path = validateArguments();
  auto filePaths = parseFilePaths();

  auto socketPath = FLAGS_socket_path.empty()
      ? getEdenSocketPath(path)
      : canonicalPath(FLAGS_socket_path);

  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  auto client = createEdenThriftClient(eventBase, socketPath);

  const PathString& mountPoint = path.asString();
  SyncBehavior sync;
  sync.syncTimeoutSeconds() = FLAGS_sync_timeout_seconds;

  size_t totalRequests = 0;
  size_t totalErrors = 0;

  for (auto _ : state) {
    auto start = std::chrono::high_resolution_clock::now();
    auto result = client->semifuture_getSHA1(mountPoint, filePaths, sync)
                      .via(eventBase)
                      .get();
    auto end = std::chrono::high_resolution_clock::now();

    benchmark::DoNotOptimize(result);

    totalRequests += result.size();
    for (const auto& entry : result) {
      if (entry.getType() == SHA1Result::Type::error) {
        ++totalErrors;
      }
    }

    auto elapsed =
        std::chrono::duration_cast<std::chrono::duration<double>>(end - start);
    state.SetIterationTime(elapsed.count());
  }

  state.counters["paths"] = static_cast<double>(filePaths.size());
  state.counters["total_requests"] = static_cast<double>(totalRequests);
  state.counters["total_errors"] = static_cast<double>(totalErrors);

  // Destroy the client on the EventBase thread to avoid
  // thread assertions in AsyncSocket/RocketClientChannel destructors.
  eventBase->runInEventBaseThreadAndWait([c = std::move(client)] {});
}

BENCHMARK(eden_get_sha1)
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16)
    ->Threads(32);

} // namespace

EDEN_BENCHMARK_MAIN();
