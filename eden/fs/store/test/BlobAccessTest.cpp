/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/BlobAccess.h"
#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/portability/GTest.h>
#include <chrono>
#include "eden/common/utils/ProcessNameCache.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/LoggingFetchContext.h"

using namespace folly::literals;
using namespace std::chrono_literals;
using namespace facebook::eden;

namespace {
const auto hash3 =
    ObjectId::fromHex("0000000000000000000000000000000000000000");
const auto hash4 =
    ObjectId::fromHex("0000000000000000000000000000000000000001");
const auto hash5 =
    ObjectId::fromHex("0000000000000000000000000000000000000002");
const auto hash6 =
    ObjectId::fromHex("0000000000000000000000000000000000000003");

const auto blob3 = std::make_shared<Blob>(hash3, "333"_sp);
const auto blob4 = std::make_shared<Blob>(hash4, "4444"_sp);
const auto blob5 = std::make_shared<Blob>(hash5, "55555"_sp);
const auto blob6 = std::make_shared<Blob>(hash6, "666666"_sp);

constexpr size_t kTreeCacheMaximumSize = 1000; // bytes
constexpr size_t kTreeCacheMinimumEntries = 0;

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
  void open() override {}
  void clearKeySpace(KeySpace) override {}
  void compactKeySpace(KeySpace) override {}

  StoreResult get(KeySpace keySpace, folly::ByteRange key) const override {
    return StoreResult::missing(keySpace, key);
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
        backingStore{std::make_shared<FakeBackingStore>()},
        blobCache{BlobCache::create(10, 0)} {
    std::shared_ptr<EdenConfig> rawEdenConfig{
        EdenConfig::createTestEdenConfig()};
    rawEdenConfig->inMemoryTreeCacheSize.setValue(
        kTreeCacheMaximumSize, ConfigSourceType::Default, true);
    rawEdenConfig->inMemoryTreeCacheMinElements.setValue(
        kTreeCacheMinimumEntries, ConfigSourceType::Default, true);
    auto edenConfig = std::make_shared<ReloadableConfig>(
        rawEdenConfig, ConfigReloadBehavior::NoReload);
    auto treeCache = TreeCache::create(edenConfig);

    localStore->open();
    objectStore = ObjectStore::create(
        localStore,
        backingStore,
        treeCache,
        std::make_shared<EdenStats>(),
        std::make_shared<ProcessNameCache>(),
        std::make_shared<NullStructuredLogger>(),
        rawEdenConfig,
        kPathMapDefaultCaseSensitive);

    blobAccess = std::make_shared<BlobAccess>(objectStore, blobCache);

    backingStore->putBlob(hash3, "333"_sp)->setReady();
    backingStore->putBlob(hash4, "4444"_sp)->setReady();
    backingStore->putBlob(hash5, "55555"_sp)->setReady();
    backingStore->putBlob(hash6, "666666"_sp)->setReady();
  }

  std::shared_ptr<const Blob> getBlobBlocking(const ObjectId& hash) {
    return blobAccess->getBlob(hash, ObjectFetchContext::getNullContext())
        .get(0ms)
        .object;
  }

  LoggingFetchContext context;
  std::shared_ptr<LocalStore> localStore;
  std::shared_ptr<FakeBackingStore> backingStore;
  std::shared_ptr<ObjectStore> objectStore;
  std::shared_ptr<BlobCache> blobCache;
  std::shared_ptr<BlobAccess> blobAccess;
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
