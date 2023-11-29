/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/experimental/TestUtil.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <stdexcept>

#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/HgRepo.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/ImmediateFuture.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

namespace {
constexpr size_t kTreeCacheMaximumSize = 1000; // bytes
constexpr size_t kTreeCacheMinimumEntries = 0;
} // namespace

struct TestRepo {
  folly::test::TemporaryDirectory testDir{"eden_hg_backing_store_test"};
  AbsolutePath testPath = canonicalPath(testDir.path().string());
  HgRepo repo{testPath + "repo"_pc};
  RootId commit1;
  Hash20 manifest1;

  TestRepo() {
    repo.hgInit(testPath + "cache"_pc);

    repo.mkdir("foo");
    repo.writeFile("foo/bar.txt", "bar\n");
    repo.mkdir("src");
    repo.writeFile("src/hello.txt", "world\n");
    repo.hg("add", "foo", "src");
    commit1 = repo.commit("Initial commit");
    manifest1 = repo.getManifestForCommit(commit1);
  }
};

struct HgBackingStoreTest : TestRepo, ::testing::Test {
  HgBackingStoreTest() {
    rawEdenConfig->inMemoryTreeCacheSize.setValue(
        kTreeCacheMaximumSize, ConfigSourceType::Default, true);
    rawEdenConfig->inMemoryTreeCacheMinimumItems.setValue(
        kTreeCacheMinimumEntries, ConfigSourceType::Default, true);
    auto treeCache = TreeCache::create(edenConfig);
    objectStore = ObjectStore::create(
        backingStore,
        treeCache,
        stats.copy(),
        std::make_shared<ProcessInfoCache>(),
        std::make_shared<NullStructuredLogger>(),
        rawEdenConfig,
        true,
        kPathMapDefaultCaseSensitive);
  }

  EdenStatsPtr stats{makeRefPtr<EdenStats>()};
  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>(stats.copy())};
  HgImporter importer{repo.path(), stats.copy()};
  std::shared_ptr<EdenConfig> rawEdenConfig{EdenConfig::createTestEdenConfig()};
  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(
          rawEdenConfig,
          ConfigReloadBehavior::NoReload)};
  FaultInjector faultInjector{/*enabled=*/false};
  std::shared_ptr<HgQueuedBackingStore> backingStore{
      std::make_shared<HgQueuedBackingStore>(
          localStore,
          stats.copy(),
          std::make_unique<HgBackingStore>(
              repo.path(),
              &importer,
              edenConfig,
              localStore,
              stats.copy(),
              &faultInjector),
          edenConfig,
          std::make_shared<NullStructuredLogger>(),
          nullptr)};
  std::shared_ptr<ObjectStore> objectStore;
};

namespace {
std::vector<PathComponent> getTreeNames(
    const std::shared_ptr<const Tree>& tree) {
  std::vector<PathComponent> names;
  for (const auto& entry : *tree) {
    names.emplace_back(entry.first);
  }
  return names;
}
} // namespace

TEST_F(
    HgBackingStoreTest,
    getTreeForCommit_reimports_tree_if_it_was_deleted_after_import) {
  auto tree1 =
      objectStore->getRootTree(commit1, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_TRUE(tree1.tree);
  ASSERT_THAT(
      getTreeNames(tree1.tree),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));

  localStore->clearKeySpace(KeySpace::TreeFamily);
  auto tree2 =
      objectStore->getRootTree(commit1, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_TRUE(tree2.tree);
  ASSERT_THAT(
      getTreeNames(tree1.tree),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));
}
