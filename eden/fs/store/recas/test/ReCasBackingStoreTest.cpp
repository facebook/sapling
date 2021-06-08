/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/executors/QueuedImmediateExecutor.h>
#include <gtest/gtest.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/recas/ReCasBackingStore.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

namespace {
const auto kTestTimeout = 10s;
}

struct ReCasBackingStoreTest : public ::testing::Test {
  ReCasBackingStoreTest() {}
  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>()};
  RootId rootId{"root"};
  Hash id = Hash::sha1("test");
  Hash manifest = Hash::sha1("manifest");
  std::unique_ptr<ReCasBackingStore> makeReCasBackingStore() {
    return std::make_unique<ReCasBackingStore>(localStore);
  }
};

TEST_F(ReCasBackingStoreTest, getRootTree) {
  auto reCasStore = makeReCasBackingStore();
  EXPECT_THROW(
      reCasStore->getRootTree(rootId, ObjectFetchContext::getNullContext())
          .via(&folly::QueuedImmediateExecutor::instance())
          .get(kTestTimeout),
      std::domain_error);
}

TEST_F(ReCasBackingStoreTest, getTree) {
  auto reCasStore = makeReCasBackingStore();
  EXPECT_THROW(
      reCasStore->getTree(id, ObjectFetchContext::getNullContext())
          .via(&folly::QueuedImmediateExecutor::instance())
          .get(),
      std::domain_error);
}

TEST_F(ReCasBackingStoreTest, getBlob) {
  auto reCasStore = makeReCasBackingStore();
  EXPECT_THROW(
      reCasStore->getBlob(id, ObjectFetchContext::getNullContext())
          .via(&folly::QueuedImmediateExecutor::instance())
          .get(kTestTimeout),
      std::domain_error);
}
