/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/io/async/EventBaseThread.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"
#include "eden/fs/utils/PathFuncs.h"
#include "watchman/cppclient/WatchmanClient.h"

DEFINE_string(query, "", "Query to run");
DEFINE_string(repo, "", "Repository to run the query against");
DEFINE_string(root, "", "Root of the query");
DEFINE_string(watchman_socket, "", "Socket to the watchman daemon");

namespace {

using namespace facebook::eden;
using namespace watchman;

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

  auto socketPath = path + ".eden/socket"_relpath;

  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  auto socket = folly::AsyncSocket::newSocket(
      eventBase, folly::SocketAddress::makeFromPath(socketPath.view()));
  auto channel =
      apache::thrift::HeaderClientChannel::newChannel(std::move(socket));
  auto client = std::make_unique<EdenServiceAsyncClient>(std::move(channel));

  GlobParams param;
  param.mountPoint_ref() = path.view();
  param.globs_ref() = std::vector<std::string>{FLAGS_query};
  param.includeDotfiles_ref() = false;
  param.prefetchFiles_ref() = false;
  param.suppressFileList_ref() = false;
  param.wantDtype_ref() = false;
  param.prefetchMetadata_ref() = false;
  param.searchRoot_ref() = FLAGS_root;

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

BENCHMARK(eden_glob)
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16)
    ->Threads(32);

BENCHMARK(watchman_glob)
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
