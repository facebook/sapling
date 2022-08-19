/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/store/test/LocalStoreTest.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"

namespace {

using namespace facebook::eden;

LocalStoreImplResult makeRocksDbLocalStore(FaultInjector* faultInjector) {
  auto tempDir = makeTempDir();
  auto store = std::make_unique<RocksDbLocalStore>(
      AbsolutePathPiece{tempDir.path().string()},
      std::make_shared<NullStructuredLogger>(),
      faultInjector);
  store->open();
  return {std::move(tempDir), std::move(store)};
}

TEST(OpenCloseRocksDBLocalStoreSemanticsTest, closeBeforeOpen) {
  auto tempDir = makeTempDir();
  auto faultInjector = FaultInjector{/*enabled=*/false};
  auto store = std::make_unique<RocksDbLocalStore>(
      AbsolutePathPiece{tempDir.path().string()},
      std::make_shared<NullStructuredLogger>(),
      &faultInjector);
  store->close();
}

TEST(OpenCloseRocksDBLocalStoreSemanticsTest, doubleClose) {
  auto tempDir = makeTempDir();
  auto faultInjector = FaultInjector{/*enabled=*/false};
  auto store = std::make_unique<RocksDbLocalStore>(
      AbsolutePathPiece{tempDir.path().string()},
      std::make_shared<NullStructuredLogger>(),
      &faultInjector);
  store->open();
  store->close();
  // no execption
  store->close();
}

void openLocalStore(std::shared_ptr<RocksDbLocalStore> store) {
  try {
    store->open();
  } catch (std::runtime_error&) {
    // sometimes the close might have happened before the open. so the open will
    // fail. thats alright.
  }
}

TEST(OpenCloseRocksDBLocalStoreSemanticsTest, closeWhileOpen) {
  auto tempDir = makeTempDir();
  auto faultInjector = FaultInjector{/*enabled=*/false};
  auto store = std::make_shared<RocksDbLocalStore>(
      AbsolutePathPiece{tempDir.path().string()},
      std::make_shared<NullStructuredLogger>(),
      &faultInjector);
  // relying on the stress testing to capture the potential interleavings here.
  std::thread openThread(openLocalStore, store);
  store->close();
  openThread.join();
}

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
INSTANTIATE_TEST_CASE_P(
    RocksDB,
    LocalStoreTest,
    ::testing::Values(makeRocksDbLocalStore));
#pragma clang diagnostic pop

} // namespace
