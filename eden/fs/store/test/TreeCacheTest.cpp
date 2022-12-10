/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/GTest.h"

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/TreeCache.h"

using namespace facebook::eden;
using namespace folly::literals;

namespace {
const auto hash0 =
    ObjectId::fromHex("0000000000000000000000000000000000000000");
const auto hash1 =
    ObjectId::fromHex("0000000000000000000000000000000000000001");
const auto hash2 =
    ObjectId::fromHex("0000000000000000000000000000000000000002");
const auto hash3 =
    ObjectId::fromHex("0000000000000000000000000000000000000003");
const auto hash4 =
    ObjectId::fromHex("0000000000000000000000000000000000000004");
const auto hash5 =
    ObjectId::fromHex("0000000000000000000000000000000000000005");
const auto hash6 =
    ObjectId::fromHex("0000000000000000000000000000000000000006");
const auto hash7 =
    ObjectId::fromHex("0000000000000000000000000000000000000007");
const auto hash8 =
    ObjectId::fromHex("0000000000000000000000000000000000000008");
const auto hash9 =
    ObjectId::fromHex("0000000000000000000000000000000000000009");

const auto entry0Name = PathComponent{"a"};
const auto entry1Name = PathComponent{"b"};
const auto entry2Name = PathComponent{"c"};
const auto entry3Name = PathComponent{"d"};
const auto entry4Name = PathComponent{"e"};

const auto entry0 = TreeEntry{hash0, TreeEntryType::REGULAR_FILE};
const auto entry1 = TreeEntry{hash1, TreeEntryType::REGULAR_FILE};
const auto entry2 = TreeEntry{hash2, TreeEntryType::REGULAR_FILE};
const auto entry3 = TreeEntry{hash3, TreeEntryType::REGULAR_FILE};
const auto entry4 = TreeEntry{hash4, TreeEntryType::REGULAR_FILE};

const auto tree0 = std::make_shared<const Tree>(
    Tree::container{{{entry0Name, entry0}}, kPathMapDefaultCaseSensitive},
    hash5);

const auto tree1 = std::make_shared<const Tree>(
    Tree::container{{{entry1Name, entry1}}, kPathMapDefaultCaseSensitive},
    hash6);

const auto tree2 = std::make_shared<const Tree>(
    Tree::container{{{entry2Name, entry2}}, kPathMapDefaultCaseSensitive},
    hash7);

const auto tree3 = std::make_shared<const Tree>(
    Tree::container{{{entry3Name, entry3}}, kPathMapDefaultCaseSensitive},
    hash8);

const auto tree4 = std::make_shared<const Tree>(
    Tree::container{
        {{entry0Name, entry0},
         {entry1Name, entry1},
         {entry2Name, entry2},
         {entry3Name, entry3},
         {entry4Name, entry4}},
        kPathMapDefaultCaseSensitive},
    hash9);

const auto entrySize = sizeof(entry0);
const auto smallTreeSize = tree0 -> getSizeBytes();
const auto bigTreeSize = tree4 -> getSizeBytes();
const auto cacheMaxSize = smallTreeSize * 3 + 1; // cache fits 3 small trees
const auto cacheMinEntries = 1; // must keep at least one tree in cache

} // namespace

struct TreeCacheTest : ::testing::Test {
 protected:
  std::shared_ptr<ReloadableConfig> edenConfig;
  std::shared_ptr<TreeCache> cache;

  void SetUp() override {
    std::shared_ptr<EdenConfig> rawEdenConfig{
        EdenConfig::createTestEdenConfig()};

    rawEdenConfig->inMemoryTreeCacheSize.setValue(
        cacheMaxSize, ConfigSourceType::Default, true);
    rawEdenConfig->inMemoryTreeCacheMinElements.setValue(
        cacheMinEntries, ConfigSourceType::Default, true);

    edenConfig = std::make_shared<ReloadableConfig>(
        rawEdenConfig, ConfigReloadBehavior::NoReload);

    cache = TreeCache::create(edenConfig);
  }
};

TEST_F(TreeCacheTest, testAssumptions) {
  // This test just exists to catch if the underlying assumptions of the rest of
  // the tests are violated rather than the caching code being incorrect. This
  // should make debugging the tests a bit easier.

  // we assume all the entries have the same size
  for (auto& entry : {entry0, entry1, entry2, entry3, entry4}) {
    EXPECT_EQ(entrySize, sizeof(entry));
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

TEST_F(TreeCacheTest, testMultipleInsert) {
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

TEST_F(TreeCacheTest, testSizeOverflowInsert) {
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

TEST_F(TreeCacheTest, testLargeInsert) {
  cache->insert(tree4);

  EXPECT_TRUE(cache->contains(tree4->getHash()));
  EXPECT_EQ(tree4, cache->get(tree4->getHash()));
}

TEST_F(TreeCacheTest, testSizeOverflowLargeInsert) {
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
