/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ObjectCache.h"
#include <gtest/gtest.h>

using namespace folly::literals;
using namespace facebook::eden;

namespace {

class CacheObject {
 public:
  const Hash& getHash() const {
    return hash_;
  }

  size_t getSizeBytes() const {
    return size_;
  }

  CacheObject(Hash hash, size_t size) : hash_{hash}, size_{size} {}

 private:
  Hash hash_;
  size_t size_;
};

constexpr auto hash3 = Hash{"0000000000000000000000000000000000000000"_sp};
constexpr auto hash4 = Hash{"0000000000000000000000000000000000000001"_sp};
constexpr auto hash5 = Hash{"0000000000000000000000000000000000000002"_sp};
constexpr auto hash6 = Hash{"0000000000000000000000000000000000000003"_sp};
constexpr auto hash9 = Hash{"0000000000000000000000000000000000000004"_sp};

// Each object's name corresponds to its length in bytes.

const auto object3 = std::make_shared<CacheObject>(hash3, 3);
const auto object4 = std::make_shared<CacheObject>(hash4, 4);
const auto object5 = std::make_shared<CacheObject>(hash5, 5);
const auto object6 = std::make_shared<CacheObject>(hash6, 6);
const auto object9 = std::make_shared<CacheObject>(hash9, 9);
} // namespace

TEST(ObjectCache, interest_handle_evicts_oldest_on_insertion) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          10, 0);
  cache->insertInterestHandle(object3);
  cache->insertInterestHandle(
      object4); // object4 is considered more recent than object3
  EXPECT_EQ(7, cache->getStats().totalSizeInBytes);
  cache->insertInterestHandle(object5); // evicts object3
  EXPECT_EQ(9, cache->getStats().totalSizeInBytes);
  EXPECT_EQ(nullptr, cache->getInterestHandle(hash3).object)
      << "Inserting object5 should evict oldest (object3)";
  EXPECT_EQ(object4, cache->getInterestHandle(hash4).object)
      << "But object4 still fits";
  cache->insertInterestHandle(object3); // evicts object5
  EXPECT_EQ(7, cache->getStats().totalSizeInBytes);
  EXPECT_EQ(nullptr, cache->getInterestHandle(hash5).object)
      << "Inserting object3 again evicts object5 because object4 was accessed";
  EXPECT_EQ(object4, cache->getInterestHandle(hash4).object);
}

TEST(
    ObjectCache,
    interest_handle_inserting_large_object_evicts_multiple_small_objects) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          10, 0);
  cache->insertInterestHandle(object3);
  cache->insertInterestHandle(object4);
  cache->insertInterestHandle(object9);
  EXPECT_FALSE(cache->getInterestHandle(hash3).object);
  EXPECT_FALSE(cache->getInterestHandle(hash4).object);
  EXPECT_EQ(object9, cache->getInterestHandle(hash9).object);
}

TEST(
    ObjectCache,
    interest_handle_inserting_existing_object_moves_it_to_back_of_eviction_queue) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(8, 0);
  cache->insertInterestHandle(object3);
  cache->insertInterestHandle(object4);
  cache->insertInterestHandle(object3);
  cache->insertInterestHandle(object5); // evicts 4

  EXPECT_EQ(object3, cache->getInterestHandle(hash3).object);
  EXPECT_FALSE(cache->getInterestHandle(hash4).object);
  EXPECT_EQ(object5, cache->getInterestHandle(hash5).object);
}

TEST(
    ObjectCache,
    interest_handle_preserves_minimum_number_of_entries_despite_exceeding_size_limit) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(1, 3);
  cache->insertInterestHandle(object3);
  cache->insertInterestHandle(object4);
  cache->insertInterestHandle(object5);

  EXPECT_EQ(12, cache->getStats().totalSizeInBytes);
  EXPECT_TRUE(cache->getInterestHandle(hash3).object);
  EXPECT_TRUE(cache->getInterestHandle(hash4).object);
  EXPECT_TRUE(cache->getInterestHandle(hash5).object);
}

TEST(ObjectCache, interest_handle_preserves_minimum_number_of_entries) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(1, 3);
  cache->insertInterestHandle(object3);
  cache->insertInterestHandle(object4);
  cache->insertInterestHandle(object5);
  cache->insertInterestHandle(object6);

  EXPECT_EQ(15, cache->getStats().totalSizeInBytes);
  EXPECT_FALSE(cache->getInterestHandle(hash3).object);
  EXPECT_TRUE(cache->getInterestHandle(hash4).object);
  EXPECT_TRUE(cache->getInterestHandle(hash5).object);
  EXPECT_TRUE(cache->getInterestHandle(hash6).object);
}

TEST(ObjectCache, interest_handle_can_forget_cached_entries) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          100, 0);
  auto handle3 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash3, 3),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  auto handle4 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash4, 4),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);

  // The use of WantHandle causes these reset() calls to evict from the cache.
  handle3.reset();
  handle4.reset();

  EXPECT_FALSE(cache->getInterestHandle(hash3).object);
  EXPECT_FALSE(cache->getInterestHandle(hash4).object);
}

TEST(
    ObjectCache,
    interest_handle_can_forget_cached_entries_in_reverse_insertion_order) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          100, 0);
  auto handle3 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash3, 3),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  auto handle4 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash4, 4),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);

  handle4.reset();
  handle3.reset();

  EXPECT_FALSE(cache->getInterestHandle(hash3).object);
  EXPECT_FALSE(cache->getInterestHandle(hash4).object);
}

TEST(ObjectCache, interest_handle_can_forget_cached_entry_in_middle) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          100, 0);
  auto handle3 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash3, 3),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  auto handle4 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash4, 4),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  auto handle5 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash5, 5),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);

  handle4.reset();

  EXPECT_TRUE(cache->getInterestHandle(hash3).object);
  EXPECT_FALSE(cache->getInterestHandle(hash4).object);
  EXPECT_TRUE(cache->getInterestHandle(hash5).object);
}

TEST(
    ObjectCache,
    interest_handle_duplicate_insertion_with_interest_forgets_on_last_drop) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          100, 0);
  auto object = std::make_shared<CacheObject>(hash3, 3);
  auto weak = std::weak_ptr<CacheObject>{object};
  auto handle1 = cache->insertInterestHandle(
      object,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  auto handle2 = cache->insertInterestHandle(
      object,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  object.reset();

  EXPECT_TRUE(weak.lock());
  handle1.reset();
  EXPECT_TRUE(weak.lock());
  handle2.reset();
  EXPECT_FALSE(weak.lock());
}

TEST(
    ObjectCache,
    interest_handle_does_not_forget_object_until_last_handle_is_forgotten) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          100, 0);
  cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash6, 6),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          UnlikelyNeededAgain);
  auto result1 = cache->getInterestHandle(
      hash6,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  auto result2 = cache->getInterestHandle(
      hash6,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  EXPECT_TRUE(result1.object);
  EXPECT_TRUE(result2.object);
  EXPECT_EQ(result1.object, result2.object);

  auto weak = std::weak_ptr<const CacheObject>{result1.object};
  result1.object.reset();
  result2.object.reset();
  EXPECT_TRUE(weak.lock());

  result1.interestHandle.reset();
  EXPECT_TRUE(weak.lock());

  result2.interestHandle.reset();
  EXPECT_FALSE(weak.lock());
}

TEST(ObjectCache, interest_handle_redundant_inserts_are_ignored) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          10, 0);
  auto object = std::make_shared<CacheObject>(Hash{}, 9);
  cache->insertInterestHandle(object);
  EXPECT_EQ(9, cache->getStats().totalSizeInBytes);
  cache->insertInterestHandle(object);
  EXPECT_EQ(9, cache->getStats().totalSizeInBytes);
  cache->insertInterestHandle(object);
  EXPECT_EQ(9, cache->getStats().totalSizeInBytes);
}

TEST(
    ObjectCache,
    interest_handle_redundant_insert_does_not_invalidate_handles) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          10, 0);
  auto handle3 = cache->insertInterestHandle(
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  cache->insertInterestHandle(
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  EXPECT_TRUE(handle3.getObject());
}

TEST(
    ObjectCache,
    interest_handle_fetching_object_from_handle_moves_to_back_of_eviction_queue) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          10, 0);
  auto handle3 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash3, 3),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  auto handle4 = cache->insertInterestHandle(
      std::make_shared<CacheObject>(hash4, 4),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);

  // Normally, inserting object5 would cause object3 to get evicted since it was
  // the first one inserted. Access object3 through its interest handle.
  EXPECT_TRUE(handle3.getObject());
  cache->insertInterestHandle(object5);
  EXPECT_TRUE(handle3.getObject());
  EXPECT_EQ(nullptr, handle4.getObject());
}

TEST(ObjectCache, interest_handle_can_return_object_even_if_it_was_evicted) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          10, 0);
  // Insert multiple objects that are never collected. Also, don't ask for
  // scoped interest.
  auto handle3 = cache->insertInterestHandle(object3);
  auto handle4 = cache->insertInterestHandle(object4);
  auto handle5 = cache->insertInterestHandle(object5);

  EXPECT_FALSE(cache->getInterestHandle(hash3).object)
      << "Inserting object5 evicts object3";
  EXPECT_EQ(object3, handle3.getObject())
      << "Object accessible even though it's been evicted";
  EXPECT_EQ(object4, handle4.getObject());
  EXPECT_EQ(object5, handle5.getObject());
}

TEST(
    ObjectCache,
    interest_handle_dropping_does_not_evict_if_item_has_been_reloaded_after_clear) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          10, 0);
  auto handle3 = cache->insertInterestHandle(
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  cache->clear();
  cache->insertInterestHandle(object3);
  handle3.reset();
  EXPECT_TRUE(cache->contains(hash3));
}

TEST(
    ObjectCache,
    dropping_interest_handle_does_not_evict_if_item_has_been_reloaded_after_eviction) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::create(
          10, 0);
  auto handle3 = cache->insertInterestHandle(
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  cache->insertInterestHandle(object4);
  cache->insertInterestHandle(object5);
  auto handle3again = cache->insertInterestHandle(
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle>::Interest::
          WantHandle);
  handle3.reset();
  EXPECT_TRUE(cache->contains(hash3));
}
