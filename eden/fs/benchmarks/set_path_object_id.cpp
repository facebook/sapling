/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <boost/uuid/random_generator.hpp>
#include <boost/uuid/uuid.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <folly/io/async/EventBaseThread.h>
#include <folly/portability/GFlags.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"
#include "eden/fs/utils/PathFuncs.h"

DEFINE_int64(threads, 1, "The number of concurrent Thrift client threads");
DEFINE_int64(path_levels, 0, "The number of folder level");
DEFINE_string(repo, "", "Path to Eden repository");
DEFINE_string(
    object_id,
    "beae1c905ff4ce5895b987b35f0365580fcb634b:4029",
    "Object id to set path to");
DEFINE_string(
    object_type,
    "regular_file",
    "Object type to set path to. support 'regular_file', 'executable_file' and 'tree' ");

namespace {
using namespace facebook::eden;

AbsolutePath validateArguments() {
  if (FLAGS_repo.empty()) {
    throw std::invalid_argument("A repo must be passed in");
  }

  return AbsolutePath{FLAGS_repo};
}

void call_set_path_object_id(benchmark::State& state) {
  auto mount = validateArguments();
  auto socketPath = mount + ".eden/socket"_relpath;

  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  auto socket = folly::AsyncSocket::newSocket(
      eventBase, folly::SocketAddress::makeFromPath(socketPath.stringPiece()));
  auto channel =
      apache::thrift::HeaderClientChannel::newChannel(std::move(socket));
  auto client = std::make_unique<EdenServiceAsyncClient>(std::move(channel));

  SetPathObjectIdParams param;
  param.mountPoint_ref() = mount.stringPiece();
  param.objectId_ref() = FLAGS_object_id;
  if ("tree" == FLAGS_object_type) {
    param.type_ref() = facebook::eden::ObjectType::TREE;
  } else if ("regular_file" == FLAGS_object_type) {
    param.type_ref() = facebook::eden::ObjectType::REGULAR_FILE;
  } else if ("executable_file" == FLAGS_object_type) {
    param.type_ref() = facebook::eden::ObjectType::EXECUTABLE_FILE;
  } else {
    throw std::invalid_argument("Unsupported object type");
  }

  auto uuidGenerator = boost::uuids::random_generator();

  std::string path = "benchmark/" + boost::uuids::to_string(uuidGenerator());
  for (long i = 0; i < FLAGS_path_levels; i++) {
    path = path + "/" + boost::uuids::to_string(uuidGenerator());
  }
  param.path_ref() = path;

  for (auto _ : state) {
    auto start = std::chrono::high_resolution_clock::now();
    auto result = client->future_setPathObjectId(param).get();
    auto end = std::chrono::high_resolution_clock::now();

    benchmark::DoNotOptimize(result);

    auto elapsed =
        std::chrono::duration_cast<std::chrono::duration<double>>(end - start);
    state.SetIterationTime(elapsed.count());
  }
}

BENCHMARK(call_set_path_object_id)
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond)
    ->Threads(1)
    ->Threads(16)
    ->Threads(64)
    ->Threads(128)
    ->Threads(512);

} // namespace

EDEN_BENCHMARK_MAIN();
