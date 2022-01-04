/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/experimental/TestUtil.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <stdexcept>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/HgRepo.h"
#include "eden/fs/utils/ProcessNameCache.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

namespace {
const auto kTestTimeout = 10s;

constexpr size_t kTreeCacheMaximumSize = 1000; // bytes
constexpr size_t kTreeCacheMinimumEntries = 0;
} // namespace

struct TestRepo {
  folly::test::TemporaryDirectory testDir{"eden_hg_backing_store_test"};
  AbsolutePath testPath{testDir.path().string()};
  HgRepo repo{testPath + "repo"_pc};
  RootId commit1;
  Hash20 manifest1;

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

/**
 * Stubbed out MetadataImporter
 */
class TestMetadataImporter : public MetadataImporter {
 public:
  TestMetadataImporter(
      std::shared_ptr<ReloadableConfig> /*config*/,
      std::string /*repoName*/,
      std::shared_ptr<LocalStore> /*localStore*/) {}

  folly::SemiFuture<std::unique_ptr<TreeMetadata>> getTreeMetadata(
      const ObjectId& /*edenId*/,
      const Hash20& /*manifestId*/) override {
    getTreeMetadataCalled = true;
    return folly::SemiFuture<std::unique_ptr<TreeMetadata>>::makeEmpty();
  }

  bool metadataFetchingAvailable() override {
    return true;
  }

  bool getTreeMetadataCalled = false;
};

class SkipMetadatPrefetchFetchContext : public ObjectFetchContext {
  bool prefetchMetadata() const override {
    return false;
  }
};

struct HgBackingStoreTest : TestRepo, ::testing::Test {
  HgBackingStoreTest() {
    rawEdenConfig->inMemoryTreeCacheSize.setValue(
        kTreeCacheMaximumSize, ConfigSource::Default, true);
    rawEdenConfig->inMemoryTreeCacheMinElements.setValue(
        kTreeCacheMinimumEntries, ConfigSource::Default, true);
    auto treeCache = TreeCache::create(edenConfig);
    objectStore = ObjectStore::create(
        localStore,
        backingStore,
        treeCache,
        stats,
        &folly::QueuedImmediateExecutor::instance(),
        std::make_shared<ProcessNameCache>(),
        std::make_shared<NullStructuredLogger>(),
        rawEdenConfig);
  }

  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>()};
  std::shared_ptr<EdenStats> stats{std::make_shared<EdenStats>()};
  HgImporter importer{repo.path(), stats};
  std::shared_ptr<EdenConfig> rawEdenConfig{EdenConfig::createTestEdenConfig()};
  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(
          rawEdenConfig,
          ConfigReloadBehavior::NoReload)};
  std::shared_ptr<HgQueuedBackingStore> backingStore{std::make_shared<
      HgQueuedBackingStore>(
      localStore,
      stats,
      std::make_unique<HgBackingStore>(
          repo.path(),
          &importer,
          edenConfig,
          localStore,
          stats,
          MetadataImporter::getMetadataImporterFactory<TestMetadataImporter>()),
      edenConfig,
      std::make_shared<NullStructuredLogger>(),
      nullptr)};
  std::shared_ptr<ObjectStore> objectStore;
};

TEST_F(
    HgBackingStoreTest,
    getTreeForCommit_reimports_tree_if_it_was_deleted_after_import) {
  auto tree1 =
      objectStore->getRootTree(commit1, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_TRUE(tree1);
  ASSERT_THAT(
      tree1->getEntryNames(),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));

  localStore->clearKeySpace(KeySpace::TreeFamily);
  auto tree2 =
      objectStore->getRootTree(commit1, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_TRUE(tree2);
  ASSERT_THAT(
      tree1->getEntryNames(),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));
}

TEST_F(HgBackingStoreTest, skipMetadataPrefetch) {
  auto metadataImporter = dynamic_cast<TestMetadataImporter*>(
      &(backingStore->getHgBackingStore().getMetadataImporter()));
  // The Metadata importer should be a TestMetadataImporter, so this should
  // never be null
  EXPECT_TRUE(metadataImporter);

  auto tree =
      objectStore->getRootTree(commit1, ObjectFetchContext::getNullContext())
          .get(0ms);
  auto context = SkipMetadatPrefetchFetchContext{};

  // Metadata prefetch should not be called here
  metadataImporter->getTreeMetadataCalled = false;
  backingStore->getTree(tree->getHash(), context).get(kTestTimeout);
  EXPECT_FALSE(metadataImporter->getTreeMetadataCalled);

  // Metadata prefetch should be called here
  metadataImporter->getTreeMetadataCalled = false;
  backingStore->getTree(tree->getHash(), ObjectFetchContext::getNullContext())
      .get(kTestTimeout);
  EXPECT_TRUE(metadataImporter->getTreeMetadataCalled);
}
