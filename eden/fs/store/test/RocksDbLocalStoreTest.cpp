/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
