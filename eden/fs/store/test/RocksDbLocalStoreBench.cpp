/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/common/testharness/TempFile.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/BlobAuxData.h"
#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace {
using namespace facebook::eden;

void getBlobAuxData(benchmark::State& st) {
  auto tempDir = makeTempDir();
  FaultInjector faultInjector{false};
  auto edenStats = makeRefPtr<EdenStats>();
  std::shared_ptr<EdenConfig> testEdenConfig =
      EdenConfig::createTestEdenConfig();
  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(testEdenConfig)};
  auto store = std::make_unique<RocksDbLocalStore>(
      canonicalPath(tempDir.path().string()),
      edenStats.copy(),
      std::make_shared<NullStructuredLogger>(),
      &faultInjector,
      edenConfig);
  store->open();

  const size_t N = 1'000'000;

  std::vector<ObjectId> ids;
  ids.reserve(N);
  for (size_t i = 0; i < N; ++i) {
    ids.emplace_back(fmt::format("{:08x}", i));
  }

  std::vector<BlobAuxData> auxData;
  auxData.reserve(N);
  for (size_t i = 0; i < N; ++i) {
    auxData.emplace_back(Hash20{}, std::nullopt, i);
  }

  for (size_t i = 0; i < N; ++i) {
    store->putBlobAuxData(ids[i], auxData[i]);
  }

  // Reopen the database to exercise the read-from-disk path.
  store.reset();
  store = std::make_unique<RocksDbLocalStore>(
      canonicalPath(tempDir.path().string()),
      edenStats.copy(),
      std::make_shared<NullStructuredLogger>(),
      &faultInjector,
      edenConfig);
  store->open();

  size_t i = 0;
  for (auto _ : st) {
    benchmark::DoNotOptimize(store->getBlobAuxData(ids[i]).get());
    if (++i == N) {
      i = 0;
    }
  }
}
BENCHMARK(getBlobAuxData);

} // namespace

EDEN_BENCHMARK_MAIN();
