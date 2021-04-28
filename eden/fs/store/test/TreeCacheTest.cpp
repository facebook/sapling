/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <gtest/gtest.h>

#include "eden/fs/model/Tree.h"
#include "eden/fs/store/TreeCache.h"

using namespace facebook::eden;
using namespace folly::literals;

namespace {
constexpr auto hash0 = Hash{"0000000000000000000000000000000000000000"_sp};
constexpr auto hash1 = Hash{"0000000000000000000000000000000000000001"_sp};
constexpr auto hash2 = Hash{"0000000000000000000000000000000000000002"_sp};
constexpr auto hash3 = Hash{"0000000000000000000000000000000000000003"_sp};
constexpr auto hash4 = Hash{"0000000000000000000000000000000000000004"_sp};
constexpr auto hash5 = Hash{"0000000000000000000000000000000000000005"_sp};
constexpr auto hash6 = Hash{"0000000000000000000000000000000000000006"_sp};
constexpr auto hash7 = Hash{"0000000000000000000000000000000000000007"_sp};
constexpr auto hash8 = Hash{"0000000000000000000000000000000000000008"_sp};
constexpr auto hash9 = Hash{"0000000000000000000000000000000000000009"_sp};

const auto entry0 =
    TreeEntry{hash0, PathComponent{"a"}, TreeEntryType::REGULAR_FILE};
const auto entry1 =
    TreeEntry{hash1, PathComponent{"b"}, TreeEntryType::REGULAR_FILE};
const auto entry2 =
    TreeEntry{hash2, PathComponent{"c"}, TreeEntryType::REGULAR_FILE};
const auto entry3 =
    TreeEntry{hash3, PathComponent{"d"}, TreeEntryType::REGULAR_FILE};
const auto entry4 =
    TreeEntry{hash4, PathComponent{"e"}, TreeEntryType::REGULAR_FILE};

const auto tree0 = std::make_shared<const Tree>(
    std::vector<TreeEntry>{
        entry0,
    },
    hash5);

const auto tree1 = std::make_shared<const Tree>(
    std::vector<TreeEntry>{
        entry1,
    },
    hash6);

const auto tree2 = std::make_shared<const Tree>(
    std::vector<TreeEntry>{
        entry2,
    },
    hash7);

const auto tree3 = std::make_shared<const Tree>(
    std::vector<TreeEntry>{
        entry3,
    },
    hash8);

const auto tree4 = std::make_shared<const Tree>(
    std::vector<TreeEntry>{entry0, entry1, entry2, entry3, entry4},
    hash9);

const auto entrySize = sizeof(entry0) + entry0.getIndirectSizeBytes();
const auto smallTreeSize = tree0 -> getSizeBytes();
const auto bigTreeSize = tree4 -> getSizeBytes();
const auto cacheMaxSize = smallTreeSize * 3 + 1; // cache fits 3 small trees
const auto cacheMinEntries = 1; // must keep at least one tree in cache

} // namespace

TEST(TreeCacheTest, testAssumptions) {
  // This test just exists to catch if the underlying assumptions of the rest of
  // the tests are violated rather than the caching code being incorrect. This
  // should make debugging the tests a bit easier.

  // we assume all the entries have the same size
  for (auto& entry : {entry0, entry1, entry2, entry3, entry4}) {
    EXPECT_EQ(entrySize, sizeof(entry) + entry.getIndirectSizeBytes());
  }

  // we assume all the little trees are the same size
  for (auto& tree : {tree1, tree2, tree3}) {
    EXPECT_EQ(tree0->getSizeBytes(), tree->getSizeBytes());
  }

  // we assume 3 small trees fit, but 4 do not.
  EXPECT_GT(cacheMaxSize, 3 * tree0->getSizeBytes());
  EXPECT_LT(cacheMaxSize, 4 * tree0->getSizeBytes());

  // we assume that the big tree is larger than the cacheSizeLimit and will only
  // be kept in the cache by the min number of entries
  EXPECT_LT(cacheMaxSize, tree4->getSizeBytes());
}

TEST(TreeCacheTest, testMultipleInsert) {
  auto cache = TreeCache::create(cacheMaxSize, cacheMinEntries);

  cache->insert(tree0);
  cache->insert(tree1);
  cache->insert(tree2);

  EXPECT_TRUE(cache->contains(tree0->getHash()));
  EXPECT_EQ(tree0, cache->get(tree0->getHash()));
  EXPECT_TRUE(cache->contains(tree1->getHash()));
  EXPECT_EQ(tree1, cache->get(tree1->getHash()));
  EXPECT_TRUE(cache->contains(tree2->getHash()));
  EXPECT_EQ(tree2, cache->get(tree2->getHash()));
}

TEST(TreeCacheTest, testSizeOverflowInsert) {
  auto cache = TreeCache::create(cacheMaxSize, cacheMinEntries);

  cache->insert(tree0);
  cache->insert(tree1);
  cache->insert(tree2);
  cache->insert(tree3);

  EXPECT_FALSE(cache->contains(tree0->getHash()));
  EXPECT_EQ(std::shared_ptr<const Tree>{nullptr}, cache->get(tree0->getHash()));
  EXPECT_TRUE(cache->contains(tree1->getHash()));
  EXPECT_EQ(tree1, cache->get(tree1->getHash()));
  EXPECT_TRUE(cache->contains(tree2->getHash()));
  EXPECT_EQ(tree2, cache->get(tree2->getHash()));
  EXPECT_TRUE(cache->contains(tree3->getHash()));
  EXPECT_EQ(tree3, cache->get(tree3->getHash()));
}

TEST(TreeCacheTest, testLargeInsert) {
  auto cache = TreeCache::create(cacheMaxSize, cacheMinEntries);

  cache->insert(tree4);

  EXPECT_TRUE(cache->contains(tree4->getHash()));
  EXPECT_EQ(tree4, cache->get(tree4->getHash()));
}

TEST(TreeCacheTest, testSizeOverflowLargeInsert) {
  auto cache = TreeCache::create(cacheMaxSize, cacheMinEntries);

  cache->insert(tree0);
  cache->insert(tree1);
  cache->insert(tree2);
  cache->insert(tree4);

  EXPECT_FALSE(cache->contains(tree0->getHash()));
  EXPECT_EQ(std::shared_ptr<const Tree>{nullptr}, cache->get(tree0->getHash()));
  EXPECT_FALSE(cache->contains(tree1->getHash()));
  EXPECT_EQ(std::shared_ptr<const Tree>{nullptr}, cache->get(tree1->getHash()));
  EXPECT_FALSE(cache->contains(tree2->getHash()));
  EXPECT_EQ(std::shared_ptr<const Tree>{nullptr}, cache->get(tree2->getHash()));
  EXPECT_TRUE(cache->contains(tree4->getHash()));
  EXPECT_EQ(tree4, cache->get(tree4->getHash()));
}
