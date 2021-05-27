/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/io/async/EventBaseThread.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>
#include "eden/fs/benchharness/Bench.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"
#include "eden/fs/utils/PathFuncs.h"
#include "watchman/cppclient/WatchmanClient.h"

DEFINE_string(query, "", "Query to run");
DEFINE_string(repo, "", "Repository to run the query against");

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

  return AbsolutePath{FLAGS_repo};
}

void eden_glob(benchmark::State& state) {
  auto path = validateArguments();

  auto socketPath = path + ".eden/socket"_relpath;

  folly::EventBase eventBase;
  auto socket = folly::AsyncSocket::newSocket(
      &eventBase, folly::SocketAddress::makeFromPath(socketPath.stringPiece()));
  auto channel =
      apache::thrift::HeaderClientChannel::newChannel(std::move(socket));
  auto client = std::make_unique<EdenServiceAsyncClient>(std::move(channel));

  GlobParams param;
  param.mountPoint_ref() = path.stringPiece();
  param.globs_ref() = std::vector<std::string>{FLAGS_query};
  param.includeDotfiles_ref() = false;
  param.prefetchFiles_ref() = false;
  param.suppressFileList_ref() = true;
  param.wantDtype_ref() = false;
  param.prefetchMetadata_ref() = false;

  auto numIterations = 0;
  for (auto _ : state) {
    Glob result;
    client->sync_globFiles(result, param);
    benchmark::DoNotOptimize(result);
    numIterations++;
  }
  state.SetItemsProcessed(numIterations);
}

void watchman_glob(benchmark::State& state) {
  auto path = validateArguments();

  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  WatchmanClient client(eventBase);
  client.connect().get();
  auto watch = client.watch(path.stringPiece()).get();

  folly::dynamic query =
      folly::dynamic::object("glob", folly::dynamic::array(FLAGS_query))(
          "fields", folly::dynamic::array());

  auto numIterations = 0;
  for (auto _ : state) {
    auto res = client.query(query, watch).get();
    benchmark::DoNotOptimize(res);
    numIterations++;
  }
  state.SetItemsProcessed(numIterations);
}

BENCHMARK(eden_glob)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16)
    ->Threads(32);

BENCHMARK(watchman_glob)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16)
    ->Threads(32);

} // namespace

EDEN_BENCHMARK_MAIN();
