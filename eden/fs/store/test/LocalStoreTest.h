/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/io/IOBuf.h>
#include <gtest/gtest.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/utils/FaultInjector.h"

namespace facebook {
namespace eden {

using LocalStoreImplResult = std::pair<
    std::optional<folly::test::TemporaryDirectory>,
    std::unique_ptr<LocalStore>>;
using LocalStoreImpl = LocalStoreImplResult (*)(FaultInjector*);

class LocalStoreTest : public ::testing::TestWithParam<LocalStoreImpl> {
 protected:
  void SetUp() override {
    auto result = GetParam()(&faultInjector_);
    testDir_ = std::move(result.first);
    store_ = std::move(result.second);
  }

  void TearDown() override {
    store_.reset();
    testDir_.reset();
  }

  FaultInjector faultInjector_{/*enabled=*/false};
  std::optional<folly::test::TemporaryDirectory> testDir_;
  std::unique_ptr<LocalStore> store_;

  using StringPiece = folly::StringPiece;
  using KeySpace = LocalStore::KeySpace;
};

} // namespace eden
} // namespace facebook
