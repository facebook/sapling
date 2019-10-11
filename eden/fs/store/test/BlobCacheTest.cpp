/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/BlobCache.h"
#include <gtest/gtest.h>
#include "eden/fs/model/Blob.h"

using namespace folly::literals;
using namespace facebook::eden;

namespace {

const auto hash3 = Hash{"0000000000000000000000000000000000000000"_sp};
const auto hash4 = Hash{"0000000000000000000000000000000000000001"_sp};
const auto hash5 = Hash{"0000000000000000000000000000000000000002"_sp};
const auto hash6 = Hash{"0000000000000000000000000000000000000003"_sp};
const auto hash9 = Hash{"0000000000000000000000000000000000000004"_sp};

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
  EXPECT_EQ(nullptr, cache->get(hash3).blob)
      << "Inserting blob5 should evict oldest (blob3)";
  EXPECT_EQ(blob4, cache->get(hash4).blob) << "But blob4 still fits";
  cache->insert(blob3); // evicts blob5
  EXPECT_EQ(7, cache->getStats().totalSizeInBytes);
  EXPECT_EQ(nullptr, cache->get(hash5).blob)
      << "Inserting blob3 again evicts blob5 because blob4 was accessed";
  EXPECT_EQ(blob4, cache->get(hash4).blob);
}

TEST(BlobCache, inserting_large_blob_evicts_multiple_small_blobs) {
  auto cache = BlobCache::create(10, 0);
  cache->insert(blob3);
  cache->insert(blob4);
  cache->insert(blob9);
  EXPECT_FALSE(cache->get(hash3).blob);
  EXPECT_FALSE(cache->get(hash4).blob);
  EXPECT_EQ(blob9, cache->get(hash9).blob);
}

TEST(BlobCache, inserting_existing_blob_moves_it_to_back_of_eviction_queue) {
  auto cache = BlobCache::create(8, 0);
  cache->insert(blob3);
  cache->insert(blob4);
  cache->insert(blob3);
  cache->insert(blob5); // evicts 4

  EXPECT_EQ(blob3, cache->get(hash3).blob);
  EXPECT_FALSE(cache->get(hash4).blob);
  EXPECT_EQ(blob5, cache->get(hash5).blob);
}

TEST(
    BlobCache,
    preserves_minimum_number_of_entries_despite_exceeding_size_limit) {
  auto cache = BlobCache::create(1, 3);
  cache->insert(blob3);
  cache->insert(blob4);
  cache->insert(blob5);

  EXPECT_EQ(12, cache->getStats().totalSizeInBytes);
  EXPECT_TRUE(cache->get(hash3).blob);
  EXPECT_TRUE(cache->get(hash4).blob);
  EXPECT_TRUE(cache->get(hash5).blob);
}

TEST(BlobCache, preserves_minimum_number_of_entries) {
  auto cache = BlobCache::create(1, 3);
  cache->insert(blob3);
  cache->insert(blob4);
  cache->insert(blob5);
  cache->insert(blob6);

  EXPECT_EQ(15, cache->getStats().totalSizeInBytes);
  EXPECT_FALSE(cache->get(hash3).blob);
  EXPECT_TRUE(cache->get(hash4).blob);
  EXPECT_TRUE(cache->get(hash5).blob);
  EXPECT_TRUE(cache->get(hash6).blob);
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

  EXPECT_FALSE(cache->get(hash3).blob);
  EXPECT_FALSE(cache->get(hash4).blob);
}

TEST(BlobCache, can_forget_cached_entries_in_reverse_insertion_order) {
  auto cache = BlobCache::create(100, 0);
  auto handle3 = cache->insert(
      std::make_shared<Blob>(hash3, "blob3"_sp),
      BlobCache::Interest::WantHandle);
  auto handle4 = cache->insert(
      std::make_shared<Blob>(hash4, "blob4"_sp),
      BlobCache::Interest::WantHandle);

  handle4.reset();
  handle3.reset();

  EXPECT_FALSE(cache->get(hash3).blob);
  EXPECT_FALSE(cache->get(hash4).blob);
}

TEST(BlobCache, can_forget_cached_entry_in_middle) {
  auto cache = BlobCache::create(100, 0);
  auto handle3 = cache->insert(
      std::make_shared<Blob>(hash3, "blob3"_sp),
      BlobCache::Interest::WantHandle);
  auto handle4 = cache->insert(
      std::make_shared<Blob>(hash4, "blob4"_sp),
      BlobCache::Interest::WantHandle);
  auto handle5 = cache->insert(
      std::make_shared<Blob>(hash5, "blob5"_sp),
      BlobCache::Interest::WantHandle);

  handle4.reset();

  EXPECT_TRUE(cache->get(hash3).blob);
  EXPECT_FALSE(cache->get(hash4).blob);
  EXPECT_TRUE(cache->get(hash5).blob);
}

TEST(BlobCache, duplicate_insertion_with_interest_forgets_on_last_drop) {
  auto cache = BlobCache::create(100, 0);
  auto blob = std::make_shared<Blob>(hash3, "blob"_sp);
  auto weak = std::weak_ptr<Blob>{blob};
  auto handle1 = cache->insert(blob, BlobCache::Interest::WantHandle);
  auto handle2 = cache->insert(blob, BlobCache::Interest::WantHandle);
  blob.reset();

  EXPECT_TRUE(weak.lock());
  handle1.reset();
  EXPECT_TRUE(weak.lock());
  handle2.reset();
  EXPECT_FALSE(weak.lock());
}

TEST(BlobCache, does_not_forget_blob_until_last_handle_is_forgotten) {
  auto cache = BlobCache::create(100, 0);
  cache->insert(
      std::make_shared<Blob>(hash6, "newblob"_sp),
      BlobCache::Interest::UnlikelyNeededAgain);
  auto result1 = cache->get(hash6, BlobCache::Interest::WantHandle);
  auto result2 = cache->get(hash6, BlobCache::Interest::WantHandle);
  EXPECT_TRUE(result1.blob);
  EXPECT_TRUE(result2.blob);
  EXPECT_EQ(result1.blob, result2.blob);

  auto weak = std::weak_ptr<const Blob>{result1.blob};
  result1.blob.reset();
  result2.blob.reset();
  EXPECT_TRUE(weak.lock());

  result1.interestHandle.reset();
  EXPECT_TRUE(weak.lock());

  result2.interestHandle.reset();
  EXPECT_FALSE(weak.lock());
}

TEST(BlobCache, redundant_inserts_are_ignored) {
  auto cache = BlobCache::create(10, 0);
  auto blob = std::make_shared<Blob>(Hash{}, "not ready"_sp);
  cache->insert(blob);
  EXPECT_EQ(9, cache->getStats().totalSizeInBytes);
  cache->insert(blob);
  EXPECT_EQ(9, cache->getStats().totalSizeInBytes);
  cache->insert(blob);
  EXPECT_EQ(9, cache->getStats().totalSizeInBytes);
}

TEST(BlobCache, redundant_insert_does_not_invalidate_interest_handles) {
  auto cache = BlobCache::create(10, 0);
  auto handle3 = cache->insert(blob3, BlobCache::Interest::WantHandle);
  cache->insert(blob3, BlobCache::Interest::WantHandle);
  EXPECT_TRUE(handle3.getBlob());
}

TEST(
    BlobCache,
    fetching_blob_from_interest_handle_moves_to_back_of_eviction_queue) {
  auto cache = BlobCache::create(10, 0);
  auto handle3 = cache->insert(
      std::make_shared<Blob>(hash3, "333"_sp), BlobCache::Interest::WantHandle);
  auto handle4 = cache->insert(
      std::make_shared<Blob>(hash4, "444"_sp), BlobCache::Interest::WantHandle);

  // Normally, inserting blob5 would cause blob3 to get evicted since it was
  // the first one inserted. Access blob3 through its interest handle.
  EXPECT_TRUE(handle3.getBlob());
  cache->insert(blob5);
  EXPECT_TRUE(handle3.getBlob());
  EXPECT_EQ(nullptr, handle4.getBlob());
}

TEST(BlobCache, interest_handle_can_return_blob_even_if_it_was_evicted) {
  auto cache = BlobCache::create(10, 0);
  // Insert multiple blobs that are never collected. Also, don't ask for scoped
  // interest.
  auto handle3 = cache->insert(blob3);
  auto handle4 = cache->insert(blob4);
  auto handle5 = cache->insert(blob5);

  EXPECT_FALSE(cache->get(hash3).blob) << "Inserting blob5 evicts blob3";
  EXPECT_EQ(blob3, handle3.getBlob())
      << "Blob accessible even though it's been evicted";
  EXPECT_EQ(blob4, handle4.getBlob());
  EXPECT_EQ(blob5, handle5.getBlob());
}

TEST(
    BlobCache,
    dropping_interest_handle_does_not_evict_if_item_has_been_reloaded_after_clear) {
  auto cache = BlobCache::create(10, 0);
  auto handle3 = cache->insert(blob3, BlobCache::Interest::WantHandle);
  cache->clear();
  cache->insert(blob3);
  handle3.reset();
  EXPECT_TRUE(cache->contains(hash3));
}

TEST(
    BlobCache,
    dropping_interest_handle_does_not_evict_if_item_has_been_reloaded_after_eviction) {
  auto cache = BlobCache::create(10, 0);
  auto handle3 = cache->insert(blob3, BlobCache::Interest::WantHandle);
  cache->insert(blob4);
  cache->insert(blob5);
  auto handle3again = cache->insert(blob3, BlobCache::Interest::WantHandle);
  handle3.reset();
  EXPECT_TRUE(cache->contains(hash3));
}
