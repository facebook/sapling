/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/io/async/EventBaseThread.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>
#include <filesystem>
#include <fstream>
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"
#include "watchman/cppclient/WatchmanClient.h"

DEFINE_string(query, "", "Query to run");
DEFINE_string(repo, "", "Repository to run the query against");
DEFINE_string(root, "", "Root of the query");
DEFINE_string(watchman_socket, "", "Socket to the watchman daemon");

namespace {

using namespace facebook::eden;
using namespace watchman;

#ifdef _WIN32
std::optional<AbsolutePath> getSocketPathFromConfig(
    const AbsolutePath& mountPath) {
  auto configPath = mountPath + ".eden/config"_relpath;

  if (!std::filesystem::exists(configPath.asString())) {
    return std::nullopt;
  }

  std::ifstream configFile(configPath.asString());
  if (!configFile.is_open()) {
    return std::nullopt;
  }

  std::string line;
  while (std::getline(configFile, line)) {
    line.erase(0, line.find_first_not_of(" \t\r\n"));
    line.erase(line.find_last_not_of(" \t\r\n") + 1);

    if (line.find("socket = ") == 0) {
      std::string socketPart = line.substr(9); // Remove "socket = "

      if (!socketPart.empty() &&
          ((socketPart.front() == '"' && socketPart.back() == '"') ||
           (socketPart.front() == '\'' && socketPart.back() == '\''))) {
        socketPart = socketPart.substr(1, socketPart.length() - 2);
      }

      try {
        return canonicalPath(socketPart);
      } catch (const std::exception&) {
        return std::nullopt;
      }
    }
  }

  return std::nullopt;
}
#endif

AbsolutePath getEdenSocketPath(const AbsolutePath& mountPath) {
#ifdef _WIN32
  // On Windows, always read from .eden/config
  auto socketPath = getSocketPathFromConfig(mountPath);
  if (socketPath) {
    return *socketPath;
  }
  throw std::runtime_error(
      "Could not find socket path in .eden/config file for Windows mount: " +
      mountPath.asString());
#else
  // On Linux and Mac, we can assume the default socket path
  return mountPath + ".eden/socket"_relpath;
#endif
}

AbsolutePath validateArguments() {
  if (FLAGS_query.empty()) {
    throw std::invalid_argument("A query argument must be passed in");
  }

  if (FLAGS_repo.empty()) {
    throw std::invalid_argument("A repo must be passed in");
  }

  return canonicalPath(FLAGS_repo);
}

void eden_glob(benchmark::State& state) {
  auto path = validateArguments();

  // Use the new socket identification logic
  auto socketPath = getEdenSocketPath(path);

  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  auto socket = folly::AsyncSocket::newSocket(
      eventBase, folly::SocketAddress::makeFromPath(socketPath.view()));
  auto channel =
      apache::thrift::HeaderClientChannel::newChannel(std::move(socket));
  auto client = std::make_unique<EdenServiceAsyncClient>(std::move(channel));

  GlobParams param;
  param.mountPoint() = path.view();
  param.globs() = std::vector<std::string>{FLAGS_query};
  param.includeDotfiles() = false;
  param.prefetchFiles() = false;
  param.suppressFileList() = false;
  param.wantDtype() = false;
  param.prefetchMetadata() = false;
  param.searchRoot() = FLAGS_root;

  for (auto _ : state) {
    auto start = std::chrono::high_resolution_clock::now();
    auto result = client->semifuture_globFiles(param).via(eventBase).get();
    auto end = std::chrono::high_resolution_clock::now();

    benchmark::DoNotOptimize(result);

    auto elapsed =
        std::chrono::duration_cast<std::chrono::duration<double>>(end - start);
    state.SetIterationTime(elapsed.count());
  }
}

BENCHMARK(eden_glob)
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16)
    ->Threads(32);

#ifndef _WIN32
// Watchman benchmark
// TODO: Figure out watchman socket connection on Windows
void watchman_glob(benchmark::State& state) {
  auto path = validateArguments();

  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  std::optional<std::string> sockPath;
  if (!FLAGS_watchman_socket.empty()) {
    sockPath = FLAGS_watchman_socket;
  }

  WatchmanClient client(eventBase, std::move(sockPath));
  client.connect().get();
  auto watch = client.watch(path.view()).get();

  folly::dynamic query =
      folly::dynamic::object("glob", folly::dynamic::array(FLAGS_query))(
          "fields", folly::dynamic::array("name"))("relative_root", FLAGS_root);

  for (auto _ : state) {
    auto start = std::chrono::high_resolution_clock::now();
    auto res = client.query(query, watch).get();
    auto end = std::chrono::high_resolution_clock::now();

    benchmark::DoNotOptimize(res);

    auto elapsed =
        std::chrono::duration_cast<std::chrono::duration<double>>(end - start);
    state.SetIterationTime(elapsed.count());
  }
}

BENCHMARK(watchman_glob)
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16)
    ->Threads(32);
#endif

} // namespace

EDEN_BENCHMARK_MAIN();
