/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/experimental/TestUtil.h>
#include <folly/logging/xlog.h>
#include <gtest/gtest.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/testharness/HgRepo.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

const auto kTestTimeout = 10s;

struct TestRepo {
  folly::test::TemporaryDirectory testDir{"eden_queued_hg_backing_store_test"};
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
    repo.writeFile("foo.txt", "foo\n");
    repo.writeFile("bar.txt", "bar\n");
    repo.hg("add");
    commit1 = repo.commit("Initial commit");
    manifest1 = repo.getManifestForCommit(commit1);
  }
};

struct HgQueuedBackingStoreTest : TestRepo, ::testing::Test {
  HgQueuedBackingStoreTest() {}

  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(EdenConfig::createTestEdenConfig())};
  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>()};
  std::shared_ptr<EdenStats> stats{std::make_shared<EdenStats>()};
  HgImporter importer{repo.path(), stats};

  std::unique_ptr<HgBackingStore> backingStore{std::make_unique<HgBackingStore>(
      repo.path(),
      &importer,
      localStore,
      stats)};

  std::unique_ptr<HgQueuedBackingStore> makeQueuedStore() {
    return std::make_unique<HgQueuedBackingStore>(
        localStore,
        stats,
        std::move(backingStore),
        std::shared_ptr<ReloadableConfig>(),
        std::make_unique<BackingStoreLogger>(),
        1);
  }
};

TEST_F(HgQueuedBackingStoreTest, getTree) {
  auto queuedStore = makeQueuedStore();
  auto tree1 = queuedStore->getTreeForCommit(commit1)
                   .via(&folly::QueuedImmediateExecutor::instance())
                   .get(kTestTimeout);

  auto tree2 =
      queuedStore
          ->getTree(tree1->getHash(), ObjectFetchContext::getNullContext())
          .via(&folly::QueuedImmediateExecutor::instance())
          .get(kTestTimeout);

  EXPECT_TRUE(*tree1 == *tree2);
}

TEST_F(HgQueuedBackingStoreTest, getBlob) {
  auto queuedStore = makeQueuedStore();
  auto tree = queuedStore->getTreeForCommit(commit1)
                  .via(&folly::QueuedImmediateExecutor::instance())
                  .get(kTestTimeout);

  for (auto& entry : tree->getTreeEntries()) {
    if (entry.getName() == "foo.txt") {
      auto blob =
          queuedStore
              ->getBlob(entry.getHash(), ObjectFetchContext::getNullContext())
              .via(&folly::QueuedImmediateExecutor::instance())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "foo\n");
    } else if (entry.getName() == "bar.txt") {
      auto blob =
          queuedStore
              ->getBlob(entry.getHash(), ObjectFetchContext::getNullContext())
              .via(&folly::QueuedImmediateExecutor::instance())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "bar\n");
    }
  }
}
