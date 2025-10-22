/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ShardedLruCache.h"
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include "eden/fs/model/ObjectId.h"

namespace {

using namespace facebook::eden;

TEST(ShardedLruCacheTest, basic_store_and_get) {
  ShardedLruCache<int> cache{4, 100};

  auto id1 = ObjectId::sha1("test1");
  auto id2 = ObjectId::sha1("test2");

  cache.store(id1, 42);
  cache.store(id2, 84);

  EXPECT_EQ(42, cache.get(id1).value());
  EXPECT_EQ(84, cache.get(id2).value());
}

TEST(ShardedLruCacheTest, get_missing_key_returns_nullopt) {
  ShardedLruCache<int> cache{4, 100};

  auto id = ObjectId::sha1("nonexistent");
  EXPECT_EQ(std::nullopt, cache.get(id));
}

TEST(ShardedLruCacheTest, overwrite_existing_key) {
  ShardedLruCache<int> cache{4, 100};

  auto id = ObjectId::sha1("test");
  cache.store(id, 42);
  cache.store(id, 100);

  EXPECT_EQ(100, cache.get(id).value());
}

TEST(ShardedLruCacheTest, eviction_on_size_limit) {
  ShardedLruCache<int> cache{1, 2};

  auto id1 = ObjectId::sha1("test1");
  auto id2 = ObjectId::sha1("test2");
  auto id3 = ObjectId::sha1("test3");

  cache.store(id1, 1);
  cache.store(id2, 2);
  cache.store(id3, 3);

  EXPECT_EQ(std::nullopt, cache.get(id1));
  EXPECT_EQ(2, cache.get(id2).value());
  EXPECT_EQ(3, cache.get(id3).value());
}

TEST(ShardedLruCacheTest, prune_hook_called_on_eviction) {
  std::vector<std::pair<ObjectId, int>> prunedItems;

  auto pruneHook = [&](const ObjectId& key, int&& value) {
    prunedItems.emplace_back(key, std::move(value));
  };

  ShardedLruCache<int> cache{1, 2, pruneHook};

  auto id1 = ObjectId::sha1("test1");
  auto id2 = ObjectId::sha1("test2");
  auto id3 = ObjectId::sha1("test3");

  cache.store(id1, 1);
  cache.store(id2, 2);
  cache.store(id3, 3);

  EXPECT_EQ(1, prunedItems.size());
  EXPECT_EQ(id1, prunedItems[0].first);
  EXPECT_EQ(1, prunedItems[0].second);
}

TEST(ShardedLruCacheTest, multiple_shards) {
  ShardedLruCache<int> cache{4, 100};

  std::vector<ObjectId> ids;
  for (int i = 0; i < 20; ++i) {
    auto id = ObjectId::sha1("test" + std::to_string(i));
    ids.push_back(id);
    cache.store(id, i);
  }

  for (int i = 0; i < 20; ++i) {
    EXPECT_EQ(i, cache.get(ids[i]).value());
  }
}

TEST(ShardedLruCacheTest, lru_ordering) {
  ShardedLruCache<int> cache{1, 3};

  auto id1 = ObjectId::sha1("test1");
  auto id2 = ObjectId::sha1("test2");
  auto id3 = ObjectId::sha1("test3");
  auto id4 = ObjectId::sha1("test4");

  cache.store(id1, 1);
  cache.store(id2, 2);
  cache.store(id3, 3);

  cache.get(id1);

  cache.store(id4, 4);

  EXPECT_TRUE(cache.get(id1).has_value());
  EXPECT_FALSE(cache.get(id2).has_value());
  EXPECT_TRUE(cache.get(id3).has_value());
  EXPECT_TRUE(cache.get(id4).has_value());
}

TEST(ShardedLruCacheTest, empty_cache) {
  ShardedLruCache<int> cache{4, 100};

  auto id = ObjectId::sha1("test");
  EXPECT_EQ(std::nullopt, cache.get(id));
}

} // namespace
