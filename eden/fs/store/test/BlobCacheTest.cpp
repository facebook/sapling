/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/BlobCache.h"
#include <folly/portability/GTest.h>
#include "eden/fs/model/Blob.h"

using namespace folly::literals;
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
const auto hash9 =
    ObjectId::fromHex("0000000000000000000000000000000000000004");

// Each blob's name corresponds to its length in bytes.

const auto blob3 = std::make_shared<Blob>(hash3, "333"_sp);
const auto blob4 = std::make_shared<Blob>(hash4, "4444"_sp);
const auto blob5 = std::make_shared<Blob>(hash5, "55555"_sp);
const auto blob6 = std::make_shared<Blob>(hash6, "666666"_sp);
const auto blob9 = std::make_shared<Blob>(hash9, "999999999"_sp);
} // namespace

TEST(BlobCache, evicts_oldest_on_insertion) {
  auto cache = BlobCache::create(10, 0);
  cache->insert(blob3);
  cache->insert(blob4); // blob4 is considered more recent than blob3
  EXPECT_EQ(7, cache->getStats().totalSizeInBytes);
  cache->insert(blob5); // evicts blob3
  EXPECT_EQ(9, cache->getStats().totalSizeInBytes);
  EXPECT_EQ(nullptr, cache->get(hash3).object)
      << "Inserting blob5 should evict oldest (blob3)";
  EXPECT_EQ(blob4, cache->get(hash4).object) << "But blob4 still fits";
  cache->insert(blob3); // evicts blob5
  EXPECT_EQ(7, cache->getStats().totalSizeInBytes);
  EXPECT_EQ(nullptr, cache->get(hash5).object)
      << "Inserting blob3 again evicts blob5 because blob4 was accessed";
  EXPECT_EQ(blob4, cache->get(hash4).object);
}

TEST(BlobCache, inserting_large_blob_evicts_multiple_small_blobs) {
  auto cache = BlobCache::create(10, 0);
  cache->insert(blob3);
  cache->insert(blob4);
  cache->insert(blob9);
  EXPECT_FALSE(cache->get(hash3).object);
  EXPECT_FALSE(cache->get(hash4).object);
  EXPECT_EQ(blob9, cache->get(hash9).object);
}

TEST(BlobCache, preserves_minimum_number_of_entries) {
  auto cache = BlobCache::create(1, 3);
  cache->insert(blob3);
  cache->insert(blob4);
  cache->insert(blob5);
  cache->insert(blob6);

  EXPECT_EQ(15, cache->getStats().totalSizeInBytes);
  EXPECT_FALSE(cache->get(hash3).object);
  EXPECT_TRUE(cache->get(hash4).object);
  EXPECT_TRUE(cache->get(hash5).object);
  EXPECT_TRUE(cache->get(hash6).object);
}

TEST(BlobCache, can_forget_cached_entries) {
  auto cache = BlobCache::create(100, 0);
  auto handle3 = cache->insert(
      std::make_shared<Blob>(hash3, "blob3"_sp),
      BlobCache::Interest::WantHandle);
  auto handle4 = cache->insert(
      std::make_shared<Blob>(hash4, "blob4"_sp),
      BlobCache::Interest::WantHandle);

  // The use of WantHandle causes these reset() calls to evict from the cache.
  handle3.reset();
  handle4.reset();

  EXPECT_FALSE(cache->get(hash3).object);
  EXPECT_FALSE(cache->get(hash4).object);
}

TEST(BlobCache, does_not_forget_blob_until_last_handle_is_forgotten) {
  auto cache = BlobCache::create(100, 0);
  auto blob = std::make_shared<Blob>(hash6, "newblob"_sp);
  auto weak = std::weak_ptr<const Blob>{blob};
  cache->insert(blob, BlobCache::Interest::UnlikelyNeededAgain);
  auto handle0 = cache->insert(blob, BlobCache::Interest::WantHandle);
  auto result1 = cache->get(hash6, BlobCache::Interest::WantHandle);
  auto result2 = cache->get(hash6, BlobCache::Interest::WantHandle);
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
