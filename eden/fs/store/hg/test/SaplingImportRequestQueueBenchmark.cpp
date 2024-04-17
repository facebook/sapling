/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <utility>
#include "eden/common/utils/IDGen.h"
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/hg/SaplingImportRequest.h"
#include "eden/fs/store/hg/SaplingImportRequestQueue.h"

namespace {

using namespace facebook::eden;

Hash20 uniqueHash() {
  std::array<uint8_t, Hash20::RAW_SIZE> bytes = {0};
  auto uid = generateUniqueID();
  std::memcpy(bytes.data(), &uid, sizeof(uid));
  return Hash20{bytes};
}

std::shared_ptr<SaplingImportRequest> makeBlobImportRequest(
    ImportPriority priority) {
  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};
  std::string proxyHashString = proxyHash.getValue();
  return SaplingImportRequest::makeBlobImportRequest(
      ObjectId{proxyHashString},
      std::move(proxyHash),
      priority,
      ObjectFetchContext::Cause::Unknown,
      std::nullopt);
}

void enqueue(benchmark::State& state) {
  auto rawEdenConfig = EdenConfig::createTestEdenConfig();
  auto edenConfig = std::make_shared<ReloadableConfig>(
      rawEdenConfig, ConfigReloadBehavior::NoReload);

  auto queue = SaplingImportRequestQueue{edenConfig};

  std::vector<std::shared_ptr<SaplingImportRequest>> requests;
  requests.reserve(state.max_iterations);
  for (size_t i = 0; i < state.max_iterations; i++) {
    requests.emplace_back(makeBlobImportRequest(kDefaultImportPriority));
  }

  auto requestIter = requests.begin();
  for (auto _ : state) {
    auto& request = *requestIter++;
    queue.enqueueBlob(std::move(request));
  }
}

void dequeue(benchmark::State& state) {
  auto rawEdenConfig = EdenConfig::createTestEdenConfig();
  auto edenConfig = std::make_shared<ReloadableConfig>(
      rawEdenConfig, ConfigReloadBehavior::NoReload);

  auto queue = SaplingImportRequestQueue{edenConfig};

  for (size_t i = 0; i < state.max_iterations; i++) {
    queue.enqueueBlob(makeBlobImportRequest(kDefaultImportPriority));
  }

  for (auto _ : state) {
    auto dequeued = queue.dequeue();
  }
}

BENCHMARK(enqueue)
    ->Unit(benchmark::kNanosecond)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16)
    ->Threads(32);

BENCHMARK(dequeue)
    ->Unit(benchmark::kNanosecond)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8)
    ->Threads(16)
    ->Threads(32);
} // namespace

EDEN_BENCHMARK_MAIN();
