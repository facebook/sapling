/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/BlobCache.h"

#include <gtest/gtest.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/telemetry/EdenStats.h"

using namespace folly::literals;
using namespace facebook::eden;

namespace {

const auto id3 = ObjectId::fromHex("0000000000000000000000000000000000000000");
const auto id4 = ObjectId::fromHex("0000000000000000000000000000000000000001");
const auto id5 = ObjectId::fromHex("0000000000000000000000000000000000000002");
const auto id6 = ObjectId::fromHex("0000000000000000000000000000000000000003");
const auto id9 = ObjectId::fromHex("0000000000000000000000000000000000000004");

// Each blob's name corresponds to its length in bytes.

const auto blob3 = std::make_shared<Blob>("333"_sp);
const auto blob4 = std::make_shared<Blob>("4444"_sp);
const auto blob5 = std::make_shared<Blob>("55555"_sp);
const auto blob6 = std::make_shared<Blob>("666666"_sp);
const auto blob9 = std::make_shared<Blob>("999999999"_sp);
} // namespace

struct BlobCacheTest : ::testing::Test {
  std::shared_ptr<ReloadableConfig> edenConfig;

  void SetUp() override {
    std::shared_ptr<EdenConfig> rawEdenConfig{
        EdenConfig::createTestEdenConfig()};

    edenConfig = std::make_shared<ReloadableConfig>(
        rawEdenConfig, ConfigReloadBehavior::NoReload);
  }
};

TEST_F(BlobCacheTest, evicts_oldest_on_insertion) {
  auto cache = BlobCache::create(10, 0, edenConfig, makeRefPtr<EdenStats>());
  cache->insert(id3, blob3);
  cache->insert(id4, blob4); // blob4 is considered more recent than blob3
  EXPECT_EQ(7, cache->getTotalSizeBytes());
  cache->insert(id5, blob5); // evicts blob3
  EXPECT_EQ(9, cache->getTotalSizeBytes());
  EXPECT_EQ(nullptr, cache->get(id3).object)
      << "Inserting blob5 should evict oldest (blob3)";
  EXPECT_EQ(blob4, cache->get(id4).object) << "But blob4 still fits";
  cache->insert(id3, blob3); // evicts blob5
  EXPECT_EQ(7, cache->getTotalSizeBytes());
  EXPECT_EQ(nullptr, cache->get(id5).object)
      << "Inserting blob3 again evicts blob5 because blob4 was accessed";
  EXPECT_EQ(blob4, cache->get(id4).object);
}

TEST_F(BlobCacheTest, inserting_large_blob_evicts_multiple_small_blobs) {
  auto cache = BlobCache::create(10, 0, edenConfig, makeRefPtr<EdenStats>());
  cache->insert(id3, blob3);
  cache->insert(id4, blob4);
  cache->insert(id9, blob9);
  EXPECT_FALSE(cache->get(id3).object);
  EXPECT_FALSE(cache->get(id4).object);
  EXPECT_EQ(blob9, cache->get(id9).object);
}

TEST_F(BlobCacheTest, preserves_minimum_number_of_entries) {
  auto cache = BlobCache::create(1, 3, edenConfig, makeRefPtr<EdenStats>());
  cache->insert(id3, blob3);
  cache->insert(id4, blob4);
  cache->insert(id5, blob5);
  cache->insert(id6, blob6);

  EXPECT_EQ(15, cache->getTotalSizeBytes());
  EXPECT_FALSE(cache->get(id3).object);
  EXPECT_TRUE(cache->get(id4).object);
  EXPECT_TRUE(cache->get(id5).object);
  EXPECT_TRUE(cache->get(id6).object);
}

TEST_F(BlobCacheTest, can_forget_cached_entries) {
  auto cache = BlobCache::create(100, 0, edenConfig, makeRefPtr<EdenStats>());
  auto handle3 = cache->insert(
      id3, std::make_shared<Blob>("blob3"_sp), BlobCache::Interest::WantHandle);
  auto handle4 = cache->insert(
      id4, std::make_shared<Blob>("blob4"_sp), BlobCache::Interest::WantHandle);

  // The use of WantHandle causes these reset() calls to evict from the cache.
  handle3.reset();
  handle4.reset();

  EXPECT_FALSE(cache->get(id3).object);
  EXPECT_FALSE(cache->get(id4).object);
}

TEST_F(BlobCacheTest, does_not_forget_blob_until_last_handle_is_forgotten) {
  auto cache = BlobCache::create(100, 0, edenConfig, makeRefPtr<EdenStats>());
  auto blob = std::make_shared<Blob>("newblob"_sp);
  auto weak = std::weak_ptr<const Blob>{blob};
  cache->insert(id6, blob, BlobCache::Interest::UnlikelyNeededAgain);
  auto handle0 = cache->insert(id6, blob, BlobCache::Interest::WantHandle);
  auto result1 = cache->get(id6, BlobCache::Interest::WantHandle);
  auto result2 = cache->get(id6, BlobCache::Interest::WantHandle);
  EXPECT_TRUE(result1.object);
  EXPECT_TRUE(result2.object);
  EXPECT_EQ(result1.object, result2.object);

  blob.reset();
  result1.object.reset();
  result2.object.reset();
  EXPECT_TRUE(weak.lock());

  handle0.reset();
  EXPECT_TRUE(weak.lock());

  result1.interestHandle.reset();
  EXPECT_TRUE(weak.lock());

  result2.interestHandle.reset();
  EXPECT_FALSE(weak.lock());
}

TEST_F(BlobCacheTest, no_blob_caching) {
  std::shared_ptr<EdenConfig> rawEdenConfig{EdenConfig::createTestEdenConfig()};
  rawEdenConfig->enableInMemoryBlobCaching.setValue(
      false, ConfigSourceType::Default, true);
  edenConfig = std::make_shared<ReloadableConfig>(
      rawEdenConfig, ConfigReloadBehavior::NoReload);
  auto cache = BlobCache::create(100, 0, edenConfig, makeRefPtr<EdenStats>());

  cache->insert(id3, blob3);
  cache->insert(id4, blob4);
  cache->insert(id5, blob5);
  // Cache should be empty since it is turned off
  EXPECT_EQ(0, cache->getTotalSizeBytes());

  auto blob = std::make_shared<Blob>("newblob"_sp);
  auto weak = std::weak_ptr<const Blob>{blob};
  auto handle = cache->insert(id6, blob, BlobCache::Interest::WantHandle);
  // Cache should be empty since it is turned off
  EXPECT_EQ(0, cache->getTotalSizeBytes());

  auto handle0 = cache->insert(id6, blob, BlobCache::Interest::WantHandle);
  // Inserting should still return the object
  EXPECT_TRUE(handle0.getObject());
  EXPECT_EQ(blob, handle0.getObject());

  // get() should always return empty
  EXPECT_FALSE(cache->get(id3).object);
  EXPECT_FALSE(cache->get(id4).object);
  EXPECT_FALSE(cache->get(id5).object);
  EXPECT_FALSE(cache->get(id6, BlobCache::Interest::WantHandle).object);
}
