/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/store/test/LocalStoreTest.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace {

using namespace facebook::eden;

LocalStoreImplResult makeRocksDbLocalStore(FaultInjector* faultInjector) {
  auto tempDir = makeTempDir();
  std::shared_ptr<EdenConfig> testEdenConfig =
      EdenConfig::createTestEdenConfig();
  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(testEdenConfig)};
  auto store = std::make_unique<RocksDbLocalStore>(
      canonicalPath(tempDir.path().string()),
      makeRefPtr<EdenStats>(),
      std::make_shared<NullStructuredLogger>(),
      faultInjector,
      edenConfig);
  return {std::move(tempDir), std::move(store)};
}

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
INSTANTIATE_TEST_CASE_P(
    RocksDB,
    LocalStoreTest,
    ::testing::Values(makeRocksDbLocalStore));

INSTANTIATE_TEST_CASE_P(
    RocksDB,
    OpenCloseLocalStoreTest,
    ::testing::Values(makeRocksDbLocalStore));
#pragma clang diagnostic pop

} // namespace
