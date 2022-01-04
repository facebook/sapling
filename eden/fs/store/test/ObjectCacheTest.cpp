/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
  const ObjectId& getHash() const {
    return hash_;
  }

  size_t getSizeBytes() const {
    return size_;
  }

  CacheObject(ObjectId hash, size_t size) : hash_{hash}, size_{size} {}

 private:
  ObjectId hash_;
  size_t size_;
};

const auto hash3 =
    ObjectId::fromHex("0000000000000000000000000000000000000000");
const auto hash3a =
    ObjectId::fromHex("0000000000000000000000000000000000000010");
const auto hash3b =
    ObjectId::fromHex("0000000000000000000000000000000000000020");
const auto hash3c =
    ObjectId::fromHex("0000000000000000000000000000000000000030");
const auto hash4 =
    ObjectId::fromHex("0000000000000000000000000000000000000001");
const auto hash5 =
    ObjectId::fromHex("0000000000000000000000000000000000000002");
const auto hash6 =
    ObjectId::fromHex("0000000000000000000000000000000000000003");
const auto hash9 =
    ObjectId::fromHex("0000000000000000000000000000000000000004");
const auto hash11 =
    ObjectId::fromHex("0000000000000000000000000000000000000005");

// Each object's name corresponds to its length in bytes.

const auto object3 = std::make_shared<CacheObject>(hash3, 3);
const auto object3a = std::make_shared<CacheObject>(hash3a, 3);
const auto object3b = std::make_shared<CacheObject>(hash3b, 3);
const auto object3c = std::make_shared<CacheObject>(hash3c, 3);
const auto object4 = std::make_shared<CacheObject>(hash4, 4);
const auto object5 = std::make_shared<CacheObject>(hash5, 5);
const auto object6 = std::make_shared<CacheObject>(hash6, 6);
const auto object9 = std::make_shared<CacheObject>(hash9, 9);
const auto object11 = std::make_shared<CacheObject>(hash11, 11);
} // namespace

/**
 * simple non-interest-handle test cases
 */

TEST(ObjectCache, testSimpleInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple>::create(10, 1);

  cache->insertSimple(object3);

  EXPECT_TRUE(cache->contains(object3->getHash()));
  EXPECT_EQ(object3, cache->getSimple(object3->getHash()));
}

TEST(ObjectCache, testMultipleInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple>::create(10, 1);

  cache->insertSimple(object3);
  cache->insertSimple(object3a);
  cache->insertSimple(object3b);

  EXPECT_TRUE(cache->contains(object3->getHash()));
  EXPECT_EQ(object3, cache->getSimple(object3->getHash()));
  EXPECT_TRUE(cache->contains(object3a->getHash()));
  EXPECT_EQ(object3a, cache->getSimple(object3a->getHash()));
  EXPECT_TRUE(cache->contains(object3b->getHash()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getHash()));
}

TEST(ObjectCache, testSizeOverflowInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple>::create(10, 1);

  cache->insertSimple(object3);
  cache->insertSimple(object3a);
  cache->insertSimple(object3b);
  cache->insertSimple(object3c);

  EXPECT_FALSE(cache->contains(object3->getHash()));
  EXPECT_EQ(
      std::shared_ptr<CacheObject>{nullptr},
      cache->getSimple(object3->getHash()));
  EXPECT_TRUE(cache->contains(object3a->getHash()));
  EXPECT_EQ(object3a, cache->getSimple(object3a->getHash()));
  EXPECT_TRUE(cache->contains(object3b->getHash()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getHash()));
  EXPECT_TRUE(cache->contains(object3c->getHash()));
  EXPECT_EQ(object3c, cache->getSimple(object3c->getHash()));
}

TEST(ObjectCache, testLRUSimpleInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple>::create(10, 1);

  cache->insertSimple(object3);
  cache->insertSimple(object3a);
  cache->insertSimple(object3b);

  cache->getSimple(object3->getHash()); // object3 should not be evicted now

  cache->insertSimple(object3c);

  EXPECT_TRUE(cache->contains(object3->getHash()));
  EXPECT_EQ(object3, cache->getSimple(object3->getHash()));
  EXPECT_FALSE(cache->contains(object3a->getHash()));
  EXPECT_EQ(
      std::shared_ptr<CacheObject>{nullptr},
      cache->getSimple(object3a->getHash()));
  EXPECT_TRUE(cache->contains(object3b->getHash()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getHash()));
  EXPECT_TRUE(cache->contains(object3c->getHash()));
  EXPECT_EQ(object3c, cache->getSimple(object3c->getHash()));
}

TEST(ObjectCache, testLargeInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple>::create(10, 1);

  cache->insertSimple(object11);

  EXPECT_TRUE(cache->contains(object11->getHash()));
  EXPECT_EQ(object11, cache->getSimple(object11->getHash()));
}

TEST(ObjectCache, testSizeOverflowLargeInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple>::create(10, 1);

  cache->insertSimple(object3);
  cache->insertSimple(object3a);
  cache->insertSimple(object3b);
  cache->insertSimple(object11);

  EXPECT_FALSE(cache->contains(object3->getHash()));
  EXPECT_EQ(
      std::shared_ptr<const CacheObject>{nullptr},
      cache->getSimple(object3->getHash()));
  EXPECT_FALSE(cache->contains(object3a->getHash()));
  EXPECT_EQ(
      std::shared_ptr<const CacheObject>{nullptr},
      cache->getSimple(object3a->getHash()));
  EXPECT_FALSE(cache->contains(object3b->getHash()));
  EXPECT_EQ(
      std::shared_ptr<const CacheObject>{nullptr},
      cache->getSimple(object3b->getHash()));
  EXPECT_TRUE(cache->contains(object11->getHash()));
  EXPECT_EQ(object11, cache->getSimple(object11->getHash()));
}

TEST(ObjectCache, testDuplicateInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple>::create(10, 1);

  cache->insertSimple(object3);
  cache->insertSimple(object3a);
  cache->insertSimple(object3b);

  cache->insertSimple(object3); // object3 should not be evicted now

  cache->insertSimple(object3c);

  EXPECT_TRUE(cache->contains(object3->getHash()));
  EXPECT_EQ(object3, cache->getSimple(object3->getHash()));
  EXPECT_FALSE(cache->contains(object3a->getHash()));
  EXPECT_EQ(
      std::shared_ptr<CacheObject>{nullptr},
      cache->getSimple(object3a->getHash()));
  EXPECT_TRUE(cache->contains(object3b->getHash()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getHash()));
  EXPECT_TRUE(cache->contains(object3c->getHash()));
  EXPECT_EQ(object3c, cache->getSimple(object3c->getHash()));
}

TEST(ObjectCache, testReinsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple>::create(10, 1);

  cache->insertSimple(object3);
  cache->insertSimple(object3a);
  cache->insertSimple(object3b);
  cache->insertSimple(object3c);
  cache->insertSimple(object3);

  EXPECT_TRUE(cache->contains(object3->getHash()));
  EXPECT_EQ(object3, cache->getSimple(object3->getHash()));
  EXPECT_FALSE(cache->contains(object3a->getHash()));
  EXPECT_EQ(
      std::shared_ptr<CacheObject>{nullptr},
      cache->getSimple(object3a->getHash()));
  EXPECT_TRUE(cache->contains(object3b->getHash()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getHash()));
  EXPECT_TRUE(cache->contains(object3c->getHash()));
  EXPECT_EQ(object3c, cache->getSimple(object3c->getHash()));
}

/**
 * Interest-handle test cases
 */

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
  auto object = std::make_shared<CacheObject>(ObjectId{}, 9);
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
