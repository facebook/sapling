/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/model/Tree.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/testharness/HgRepo.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

struct TestRepo {
  folly::test::TemporaryDirectory testDir{"eden_hg_backing_store_test"};
  AbsolutePath testPath{testDir.path().string()};
  HgRepo repo{testPath + "repo"_pc};
  Hash commit1;
  Hash manifest1;

  TestRepo() {
    repo.hgInit();
    repo.enableTreeManifest(testPath + "cache"_pc);

    repo.mkdir("foo");
    repo.writeFile("foo/bar.txt", "bar\n");
    repo.mkdir("src");
    repo.writeFile("src/hello.txt", "world\n");
    repo.hg("add");
    commit1 = repo.commit("Initial commit");
    manifest1 = repo.getManifestForCommit(commit1);
  }
};

struct HgBackingStoreTest : TestRepo, ::testing::Test {
  HgBackingStoreTest() {}

  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>()};
  std::shared_ptr<EdenStats> stats{std::make_shared<EdenStats>()};
  HgImporter importer{repo.path(),
                      localStore.get(),
                      getSharedHgImporterStatsForCurrentThread(stats)};
  std::shared_ptr<HgBackingStore> backingStore{std::make_shared<HgBackingStore>(
      repo.path(),
      &importer,
      localStore.get(),
      stats)};
  std::shared_ptr<ObjectStore> objectStore{
      ObjectStore::create(localStore, backingStore, stats)};
};

TEST_F(
    HgBackingStoreTest,
    getTreeForCommit_reimports_tree_if_it_was_deleted_after_import) {
  auto tree1 = objectStore->getTreeForCommit(commit1).get(0ms);
  EXPECT_TRUE(tree1);
  ASSERT_THAT(
      tree1->getEntryNames(),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));

  localStore->clearKeySpace(LocalStore::TreeFamily);
  auto tree2 = objectStore->getTreeForCommit(commit1).get(0ms);
  EXPECT_TRUE(tree2);
  ASSERT_THAT(
      tree1->getEntryNames(),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));
}

TEST_F(HgBackingStoreTest, getTreeForManifest) {
  auto tree1 = objectStore->getTreeForCommit(commit1).get(0ms);
  auto tree2 = objectStore->getTreeForManifest(commit1, manifest1).get(0ms);
  EXPECT_EQ(tree1->getHash(), tree2->getHash());
}
