/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/BlobAccess.h"
#include <folly/executors/QueuedImmediateExecutor.h>
#include <gtest/gtest.h>
#include <chrono>
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/LoggingFetchContext.h"
#ifndef _WIN32
#include "eden/fs/utils/ProcessNameCache.h"
#else
#include "eden/fs/win/utils/Stub.h" // @manual
#endif
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"

using namespace folly::literals;
using namespace std::chrono_literals;
using namespace facebook::eden;

namespace {
const auto hash3 = Hash{"0000000000000000000000000000000000000000"_sp};
const auto hash4 = Hash{"0000000000000000000000000000000000000001"_sp};
const auto hash5 = Hash{"0000000000000000000000000000000000000002"_sp};
const auto hash6 = Hash{"0000000000000000000000000000000000000003"_sp};

const auto blob3 = std::make_shared<Blob>(hash3, "333"_sp);
const auto blob4 = std::make_shared<Blob>(hash4, "4444"_sp);
const auto blob5 = std::make_shared<Blob>(hash5, "55555"_sp);
const auto blob6 = std::make_shared<Blob>(hash6, "666666"_sp);

/**
 * These tests attempt to measure the number of hits to the backing store, so
 * prevent anything from getting cached in the local store.
 */
class NullLocalStore final : public LocalStore {
 public:
  class NullWriteBatch final : public LocalStore::WriteBatch {
   public:
    void put(KeySpace, folly::ByteRange, folly::ByteRange) override {}
    void put(KeySpace, folly::ByteRange, std::vector<folly::ByteRange>)
        override {}
    void flush() override {}
  };

  void close() override {}
  void clearKeySpace(KeySpace) override {}
  void compactKeySpace(KeySpace) override {}

  StoreResult get(KeySpace, folly::ByteRange) const override {
    return StoreResult{};
  }

  bool hasKey(KeySpace, folly::ByteRange) const override {
    return false;
  }

  void put(KeySpace, folly::ByteRange, folly::ByteRange) override {}

  std::unique_ptr<WriteBatch> beginWrite(size_t) override {
    return std::make_unique<NullWriteBatch>();
  }
};

struct BlobAccessTest : ::testing::Test {
  BlobAccessTest()
      : localStore{std::make_shared<NullLocalStore>()},
        backingStore{std::make_shared<FakeBackingStore>(localStore)},
        objectStore{ObjectStore::create(
            localStore,
            backingStore,
            std::make_shared<EdenStats>(),
            &folly::QueuedImmediateExecutor::instance(),
            std::make_shared<ProcessNameCache>(),
            std::make_shared<NullStructuredLogger>(),
            EdenConfig::createTestEdenConfig())},
        blobCache{BlobCache::create(10, 0)},
        blobAccess{objectStore, blobCache} {
    backingStore->putBlob(hash3, "333"_sp)->setReady();
    backingStore->putBlob(hash4, "4444"_sp)->setReady();
    backingStore->putBlob(hash5, "55555"_sp)->setReady();
    backingStore->putBlob(hash6, "666666"_sp)->setReady();
  }

  std::shared_ptr<const Blob> getBlobBlocking(const Hash& hash) {
    return blobAccess.getBlob(hash, ObjectFetchContext::getNullContext())
        .get(0ms)
        .blob;
  }

  LoggingFetchContext context;
  std::shared_ptr<LocalStore> localStore;
  std::shared_ptr<FakeBackingStore> backingStore;
  std::shared_ptr<ObjectStore> objectStore;
  std::shared_ptr<BlobCache> blobCache;
  BlobAccess blobAccess;
};

} // namespace

TEST_F(BlobAccessTest, remembers_blobs) {
  auto blob1 = getBlobBlocking(hash4);
  auto blob2 = getBlobBlocking(hash4);

  EXPECT_EQ(blob1, blob2);
  EXPECT_EQ(4, blob1->getSize());
  EXPECT_EQ(1, backingStore->getAccessCount(hash4));
}

TEST_F(BlobAccessTest, drops_blobs_when_size_is_exceeded) {
  auto blob1 = getBlobBlocking(hash6);
  auto blob2 = getBlobBlocking(hash5);
  auto blob3 = getBlobBlocking(hash6);

  EXPECT_EQ(6, blob1->getSize());
  EXPECT_EQ(5, blob2->getSize());
  EXPECT_EQ(6, blob3->getSize());

  EXPECT_EQ(1, backingStore->getAccessCount(hash5));
  EXPECT_EQ(2, backingStore->getAccessCount(hash6));
}

TEST_F(BlobAccessTest, drops_oldest_blobs) {
  getBlobBlocking(hash3);
  getBlobBlocking(hash4);

  // Evicts hash3
  getBlobBlocking(hash5);
  EXPECT_EQ(1, backingStore->getAccessCount(hash3));
  EXPECT_EQ(1, backingStore->getAccessCount(hash4));
  EXPECT_EQ(1, backingStore->getAccessCount(hash5));

  // Evicts hash4 but not hash5
  getBlobBlocking(hash3);
  getBlobBlocking(hash5);
  EXPECT_EQ(2, backingStore->getAccessCount(hash3));
  EXPECT_EQ(1, backingStore->getAccessCount(hash4));
  EXPECT_EQ(1, backingStore->getAccessCount(hash5));

  // Evicts hash3
  getBlobBlocking(hash4);
  getBlobBlocking(hash5);
  EXPECT_EQ(2, backingStore->getAccessCount(hash3));
  EXPECT_EQ(2, backingStore->getAccessCount(hash4));
  EXPECT_EQ(1, backingStore->getAccessCount(hash5));
}
