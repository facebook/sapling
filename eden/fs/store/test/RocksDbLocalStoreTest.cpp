/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/store/test/LocalStoreTest.h"

namespace {

using namespace facebook::eden;

LocalStoreImplResult makeRocksDbLocalStore(FaultInjector* faultInjector) {
  auto tempDir = makeTempDir();
  auto store = std::make_unique<RocksDbLocalStore>(
      AbsolutePathPiece{tempDir.path().string()}, faultInjector);
  return {std::move(tempDir), std::move(store)};
}

INSTANTIATE_TEST_CASE_P(
    RocksDB,
    LocalStoreTest,
    ::testing::Values(makeRocksDbLocalStore));

} // namespace
