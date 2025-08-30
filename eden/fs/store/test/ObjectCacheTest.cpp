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
  const ObjectId& getObjectId() const {
    return id_;
  }

  size_t getSizeBytes() const {
    return size_;
  }

  CacheObject(ObjectId id, size_t size) : id_{id}, size_{size} {}

 private:
  ObjectId id_;
  size_t size_;
};

const auto id3 = ObjectId::fromHex("0000000000000000000000000000000000000000");
const auto id3a = ObjectId::fromHex("0000000000000000000000000000000000000010");
const auto id3b = ObjectId::fromHex("0000000000000000000000000000000000000020");
const auto id3c = ObjectId::fromHex("0000000000000000000000000000000000000030");
const auto id4 = ObjectId::fromHex("0000000000000000000000000000000000000001");
const auto id5 = ObjectId::fromHex("0000000000000000000000000000000000000002");
const auto id6 = ObjectId::fromHex("0000000000000000000000000000000000000003");
const auto id9 = ObjectId::fromHex("0000000000000000000000000000000000000004");
const auto id11 = ObjectId::fromHex("0000000000000000000000000000000000000005");

// Each object's name corresponds to its length in bytes.

const auto object3 = std::make_shared<CacheObject>(id3, 3);
const auto object3a = std::make_shared<CacheObject>(id3a, 3);
const auto object3b = std::make_shared<CacheObject>(id3b, 3);
const auto object3c = std::make_shared<CacheObject>(id3c, 3);
const auto object4 = std::make_shared<CacheObject>(id4, 4);
const auto object5 = std::make_shared<CacheObject>(id5, 5);
const auto object6 = std::make_shared<CacheObject>(id6, 6);
const auto object9 = std::make_shared<CacheObject>(id9, 9);
const auto object11 = std::make_shared<CacheObject>(id11, 11);
} // namespace

/**
 * simple non-interest-handle test cases
 */

TEST(ObjectCache, testSimpleInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple, FakeStats>::create(
          10, 1, makeRefPtr<EdenStats>());

  cache->insertSimple(object3->getObjectId(), object3);

  EXPECT_TRUE(cache->contains(object3->getObjectId()));
  EXPECT_EQ(object3, cache->getSimple(object3->getObjectId()));
}

TEST(ObjectCache, testMultipleInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple, FakeStats>::create(
          10, 1, makeRefPtr<EdenStats>());

  cache->insertSimple(object3->getObjectId(), object3);
  cache->insertSimple(object3a->getObjectId(), object3a);
  cache->insertSimple(object3b->getObjectId(), object3b);

  EXPECT_TRUE(cache->contains(object3->getObjectId()));
  EXPECT_EQ(object3, cache->getSimple(object3->getObjectId()));
  EXPECT_TRUE(cache->contains(object3a->getObjectId()));
  EXPECT_EQ(object3a, cache->getSimple(object3a->getObjectId()));
  EXPECT_TRUE(cache->contains(object3b->getObjectId()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getObjectId()));
}

TEST(ObjectCache, testSizeOverflowInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple, FakeStats>::create(
          10, 1, makeRefPtr<EdenStats>());

  cache->insertSimple(object3->getObjectId(), object3);
  cache->insertSimple(object3a->getObjectId(), object3a);
  cache->insertSimple(object3b->getObjectId(), object3b);
  cache->insertSimple(object3c->getObjectId(), object3c);

  EXPECT_FALSE(cache->contains(object3->getObjectId()));
  EXPECT_EQ(
      std::shared_ptr<CacheObject>{nullptr},
      cache->getSimple(object3->getObjectId()));
  EXPECT_TRUE(cache->contains(object3a->getObjectId()));
  EXPECT_EQ(object3a, cache->getSimple(object3a->getObjectId()));
  EXPECT_TRUE(cache->contains(object3b->getObjectId()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getObjectId()));
  EXPECT_TRUE(cache->contains(object3c->getObjectId()));
  EXPECT_EQ(object3c, cache->getSimple(object3c->getObjectId()));
}

TEST(ObjectCache, testLRUSimpleInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple, FakeStats>::create(
          10, 1, makeRefPtr<EdenStats>());

  cache->insertSimple(object3->getObjectId(), object3);
  cache->insertSimple(object3a->getObjectId(), object3a);
  cache->insertSimple(object3b->getObjectId(), object3b);

  cache->getSimple(object3->getObjectId()); // object3 should not be evicted now

  cache->insertSimple(object3c->getObjectId(), object3c);

  EXPECT_TRUE(cache->contains(object3->getObjectId()));
  EXPECT_EQ(object3, cache->getSimple(object3->getObjectId()));
  EXPECT_FALSE(cache->contains(object3a->getObjectId()));
  EXPECT_EQ(
      std::shared_ptr<CacheObject>{nullptr},
      cache->getSimple(object3a->getObjectId()));
  EXPECT_TRUE(cache->contains(object3b->getObjectId()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getObjectId()));
  EXPECT_TRUE(cache->contains(object3c->getObjectId()));
  EXPECT_EQ(object3c, cache->getSimple(object3c->getObjectId()));
}

TEST(ObjectCache, testLargeInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple, FakeStats>::create(
          10, 1, makeRefPtr<EdenStats>());

  cache->insertSimple(object11->getObjectId(), object11);

  EXPECT_TRUE(cache->contains(object11->getObjectId()));
  EXPECT_EQ(object11, cache->getSimple(object11->getObjectId()));
}

TEST(ObjectCache, testSizeOverflowLargeInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple, FakeStats>::create(
          10, 1, makeRefPtr<EdenStats>());

  cache->insertSimple(object3->getObjectId(), object3);
  cache->insertSimple(object3a->getObjectId(), object3a);
  cache->insertSimple(object3b->getObjectId(), object3b);
  cache->insertSimple(object11->getObjectId(), object11);

  EXPECT_FALSE(cache->contains(object3->getObjectId()));
  EXPECT_EQ(
      std::shared_ptr<const CacheObject>{nullptr},
      cache->getSimple(object3->getObjectId()));
  EXPECT_FALSE(cache->contains(object3a->getObjectId()));
  EXPECT_EQ(
      std::shared_ptr<const CacheObject>{nullptr},
      cache->getSimple(object3a->getObjectId()));
  EXPECT_FALSE(cache->contains(object3b->getObjectId()));
  EXPECT_EQ(
      std::shared_ptr<const CacheObject>{nullptr},
      cache->getSimple(object3b->getObjectId()));
  EXPECT_TRUE(cache->contains(object11->getObjectId()));
  EXPECT_EQ(object11, cache->getSimple(object11->getObjectId()));
}

TEST(ObjectCache, testDuplicateInsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple, FakeStats>::create(
          10, 1, makeRefPtr<EdenStats>());

  cache->insertSimple(object3->getObjectId(), object3);
  cache->insertSimple(object3a->getObjectId(), object3a);
  cache->insertSimple(object3b->getObjectId(), object3b);

  cache->insertSimple(
      object3->getObjectId(), object3); // object3 should not be evicted now

  cache->insertSimple(object3c->getObjectId(), object3c);

  EXPECT_TRUE(cache->contains(object3->getObjectId()));
  EXPECT_EQ(object3, cache->getSimple(object3->getObjectId()));
  EXPECT_FALSE(cache->contains(object3a->getObjectId()));
  EXPECT_EQ(
      std::shared_ptr<CacheObject>{nullptr},
      cache->getSimple(object3a->getObjectId()));
  EXPECT_TRUE(cache->contains(object3b->getObjectId()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getObjectId()));
  EXPECT_TRUE(cache->contains(object3c->getObjectId()));
  EXPECT_EQ(object3c, cache->getSimple(object3c->getObjectId()));
}

TEST(ObjectCache, testReinsert) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::Simple, FakeStats>::create(
          10, 1, makeRefPtr<EdenStats>());

  cache->insertSimple(object3->getObjectId(), object3);
  cache->insertSimple(object3a->getObjectId(), object3a);
  cache->insertSimple(object3b->getObjectId(), object3b);
  cache->insertSimple(object3c->getObjectId(), object3c);
  cache->insertSimple(object3->getObjectId(), object3);

  EXPECT_TRUE(cache->contains(object3->getObjectId()));
  EXPECT_EQ(object3, cache->getSimple(object3->getObjectId()));
  EXPECT_FALSE(cache->contains(object3a->getObjectId()));
  EXPECT_EQ(
      std::shared_ptr<CacheObject>{nullptr},
      cache->getSimple(object3a->getObjectId()));
  EXPECT_TRUE(cache->contains(object3b->getObjectId()));
  EXPECT_EQ(object3b, cache->getSimple(object3b->getObjectId()));
  EXPECT_TRUE(cache->contains(object3c->getObjectId()));
  EXPECT_EQ(object3c, cache->getSimple(object3c->getObjectId()));
}

/**
 * Interest-handle test cases
 */

TEST(ObjectCache, interest_handle_evicts_oldest_on_insertion) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(10, 0, makeRefPtr<EdenStats>());
  cache->insertInterestHandle(object3->getObjectId(), object3);
  cache->insertInterestHandle(
      object4->getObjectId(),
      object4); // object4 is considered more recent than object3
  EXPECT_EQ(7, cache->getTotalSizeBytes());
  cache->insertInterestHandle(
      object5->getObjectId(), object5); // evicts object3
  EXPECT_EQ(9, cache->getTotalSizeBytes());
  EXPECT_EQ(nullptr, cache->getInterestHandle(id3).object)
      << "Inserting object5 should evict oldest (object3)";
  EXPECT_EQ(object4, cache->getInterestHandle(id4).object)
      << "But object4 still fits";
  cache->insertInterestHandle(
      object3->getObjectId(), object3); // evicts object5
  EXPECT_EQ(7, cache->getTotalSizeBytes());
  EXPECT_EQ(nullptr, cache->getInterestHandle(id5).object)
      << "Inserting object3 again evicts object5 because object4 was accessed";
  EXPECT_EQ(object4, cache->getInterestHandle(id4).object);
}

TEST(
    ObjectCache,
    interest_handle_inserting_large_object_evicts_multiple_small_objects) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(10, 0, makeRefPtr<EdenStats>());
  cache->insertInterestHandle(object3->getObjectId(), object3);
  cache->insertInterestHandle(object4->getObjectId(), object4);
  cache->insertInterestHandle(object9->getObjectId(), object9);
  EXPECT_FALSE(cache->getInterestHandle(id3).object);
  EXPECT_FALSE(cache->getInterestHandle(id4).object);
  EXPECT_EQ(object9, cache->getInterestHandle(id9).object);
}

TEST(
    ObjectCache,
    interest_handle_inserting_existing_object_moves_it_to_back_of_eviction_queue) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(8, 0, makeRefPtr<EdenStats>());
  cache->insertInterestHandle(object3->getObjectId(), object3);
  cache->insertInterestHandle(object4->getObjectId(), object4);
  cache->insertInterestHandle(object3->getObjectId(), object3);
  cache->insertInterestHandle(object5->getObjectId(), object5); // evicts 4

  EXPECT_EQ(object3, cache->getInterestHandle(id3).object);
  EXPECT_FALSE(cache->getInterestHandle(id4).object);
  EXPECT_EQ(object5, cache->getInterestHandle(id5).object);
}

TEST(
    ObjectCache,
    interest_handle_preserves_minimum_number_of_entries_despite_exceeding_size_limit) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(1, 3, makeRefPtr<EdenStats>());
  cache->insertInterestHandle(object3->getObjectId(), object3);
  cache->insertInterestHandle(object4->getObjectId(), object4);
  cache->insertInterestHandle(object5->getObjectId(), object5);

  EXPECT_EQ(12, cache->getTotalSizeBytes());
  EXPECT_TRUE(cache->getInterestHandle(id3).object);
  EXPECT_TRUE(cache->getInterestHandle(id4).object);
  EXPECT_TRUE(cache->getInterestHandle(id5).object);
}

TEST(ObjectCache, interest_handle_preserves_minimum_number_of_entries) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(1, 3, makeRefPtr<EdenStats>());
  cache->insertInterestHandle(object3->getObjectId(), object3);
  cache->insertInterestHandle(object4->getObjectId(), object4);
  cache->insertInterestHandle(object5->getObjectId(), object5);
  cache->insertInterestHandle(object6->getObjectId(), object6);

  EXPECT_EQ(15, cache->getTotalSizeBytes());
  EXPECT_FALSE(cache->getInterestHandle(id3).object);
  EXPECT_TRUE(cache->getInterestHandle(id4).object);
  EXPECT_TRUE(cache->getInterestHandle(id5).object);
  EXPECT_TRUE(cache->getInterestHandle(id6).object);
}

TEST(ObjectCache, interest_handle_can_forget_cached_entries) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(100, 0, makeRefPtr<EdenStats>());
  auto handle3 = cache->insertInterestHandle(
      id3,
      std::make_shared<CacheObject>(id3, 3),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  auto handle4 = cache->insertInterestHandle(
      id4,
      std::make_shared<CacheObject>(id4, 4),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);

  // The use of WantHandle causes these reset() calls to evict from the cache.
  handle3.reset();
  handle4.reset();

  EXPECT_FALSE(cache->getInterestHandle(id3).object);
  EXPECT_FALSE(cache->getInterestHandle(id4).object);
}

TEST(
    ObjectCache,
    interest_handle_can_forget_cached_entries_in_reverse_insertion_order) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(100, 0, makeRefPtr<EdenStats>());
  auto handle3 = cache->insertInterestHandle(
      id3,
      std::make_shared<CacheObject>(id3, 3),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  auto handle4 = cache->insertInterestHandle(
      id4,
      std::make_shared<CacheObject>(id4, 4),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);

  handle4.reset();
  handle3.reset();

  EXPECT_FALSE(cache->getInterestHandle(id3).object);
  EXPECT_FALSE(cache->getInterestHandle(id4).object);
}

TEST(ObjectCache, interest_handle_can_forget_cached_entry_in_middle) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(100, 0, makeRefPtr<EdenStats>());
  auto handle3 = cache->insertInterestHandle(
      id3,
      std::make_shared<CacheObject>(id3, 3),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  auto handle4 = cache->insertInterestHandle(
      id4,
      std::make_shared<CacheObject>(id4, 4),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  auto handle5 = cache->insertInterestHandle(
      id5,
      std::make_shared<CacheObject>(id5, 5),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);

  handle4.reset();

  EXPECT_TRUE(cache->getInterestHandle(id3).object);
  EXPECT_FALSE(cache->getInterestHandle(id4).object);
  EXPECT_TRUE(cache->getInterestHandle(id5).object);
}

TEST(
    ObjectCache,
    interest_handle_duplicate_insertion_with_interest_forgets_on_last_drop) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(100, 0, makeRefPtr<EdenStats>());
  auto object = std::make_shared<CacheObject>(id3, 3);
  auto weak = std::weak_ptr<CacheObject>{object};
  auto handle1 = cache->insertInterestHandle(
      object->getObjectId(),
      object,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  auto handle2 = cache->insertInterestHandle(
      object->getObjectId(),
      object,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
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
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(100, 0, makeRefPtr<EdenStats>());
  cache->insertInterestHandle(
      id6,
      std::make_shared<CacheObject>(id6, 6),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::UnlikelyNeededAgain);
  auto result1 = cache->getInterestHandle(
      id6,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  auto result2 = cache->getInterestHandle(
      id6,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
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
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(10, 0, makeRefPtr<EdenStats>());
  auto object = std::make_shared<CacheObject>(ObjectId{}, 9);
  cache->insertInterestHandle(object->getObjectId(), object);
  EXPECT_EQ(9, cache->getTotalSizeBytes());
  cache->insertInterestHandle(object->getObjectId(), object);
  EXPECT_EQ(9, cache->getTotalSizeBytes());
  cache->insertInterestHandle(object->getObjectId(), object);
  EXPECT_EQ(9, cache->getTotalSizeBytes());
}

TEST(
    ObjectCache,
    interest_handle_redundant_insert_does_not_invalidate_handles) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(10, 0, makeRefPtr<EdenStats>());
  auto handle3 = cache->insertInterestHandle(
      object3->getObjectId(),
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  cache->insertInterestHandle(
      object3->getObjectId(),
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  EXPECT_TRUE(handle3.getObject());
}

TEST(
    ObjectCache,
    interest_handle_fetching_object_from_handle_moves_to_back_of_eviction_queue) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(10, 0, makeRefPtr<EdenStats>());
  auto handle3 = cache->insertInterestHandle(
      id3,
      std::make_shared<CacheObject>(id3, 3),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  auto handle4 = cache->insertInterestHandle(
      id4,
      std::make_shared<CacheObject>(id4, 4),
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);

  // Normally, inserting object5 would cause object3 to get evicted since it was
  // the first one inserted. Access object3 through its interest handle.
  EXPECT_TRUE(handle3.getObject());
  cache->insertInterestHandle(object5->getObjectId(), object5);
  EXPECT_TRUE(handle3.getObject());
  EXPECT_EQ(nullptr, handle4.getObject());
}

TEST(ObjectCache, interest_handle_can_return_object_even_if_it_was_evicted) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(10, 0, makeRefPtr<EdenStats>());
  // Insert multiple objects that are never collected. Also, don't ask for
  // scoped interest.
  auto handle3 = cache->insertInterestHandle(object3->getObjectId(), object3);
  auto handle4 = cache->insertInterestHandle(object4->getObjectId(), object4);
  auto handle5 = cache->insertInterestHandle(object5->getObjectId(), object5);

  EXPECT_FALSE(cache->getInterestHandle(id3).object)
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
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(10, 0, makeRefPtr<EdenStats>());
  auto handle3 = cache->insertInterestHandle(
      object3->getObjectId(),
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  cache->clear();
  cache->insertInterestHandle(object3->getObjectId(), object3);
  handle3.reset();
  EXPECT_TRUE(cache->contains(id3));
}

TEST(
    ObjectCache,
    dropping_interest_handle_does_not_evict_if_item_has_been_reloaded_after_eviction) {
  auto cache =
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          create(10, 0, makeRefPtr<EdenStats>());
  auto handle3 = cache->insertInterestHandle(
      object3->getObjectId(),
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  cache->insertInterestHandle(object4->getObjectId(), object4);
  cache->insertInterestHandle(object5->getObjectId(), object5);
  auto handle3again = cache->insertInterestHandle(
      object3->getObjectId(),
      object3,
      ObjectCache<CacheObject, ObjectCacheFlavor::InterestHandle, FakeStats>::
          Interest::WantHandle);
  handle3.reset();
  EXPECT_TRUE(cache->contains(id3));
}
