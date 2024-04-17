/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/experimental/TestUtil.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <algorithm>
#include <memory>

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/hg/SaplingBackingStore.h"
#include "eden/fs/store/hg/SaplingBackingStoreOptions.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/testharness/HgRepo.h"
#include "eden/fs/testharness/TestConfigSource.h"

using namespace std::chrono_literals;

namespace facebook::eden {
namespace {
const auto kTestTimeout = 10s;

struct TestRepo {
  folly::test::TemporaryDirectory testDir{"eden_queued_hg_backing_store_test"};
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
    repo.writeFile("foo.txt", "foo\n");
    repo.writeFile("bar.txt", "bar\n");
    repo.hg("add", "foo/bar.txt", "src/hello.txt", "foo.txt", "bar.txt");
    commit1 = repo.commit("Initial commit");
    manifest1 = repo.getManifestForCommit(commit1);
  }
};

std::vector<PathComponent> getTreeNames(
    const std::shared_ptr<const Tree>& tree) {
  std::vector<PathComponent> names;
  for (const auto& entry : *tree) {
    if (entry.second.isTree()) {
      names.emplace_back(entry.first);
    }
  }
  return names;
}

struct SaplingBackingStoreTestBase : TestRepo, ::testing::Test {
  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(EdenConfig::createTestEdenConfig())};
  EdenStatsPtr stats{makeRefPtr<EdenStats>()};
  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>(stats.copy())};
};

struct SaplingBackingStoreNoFaultInjectorTest : SaplingBackingStoreTestBase {
  FaultInjector faultInjector{/*enabled=*/false};

  std::unique_ptr<SaplingBackingStore> queuedBackingStore =
      std::make_unique<SaplingBackingStore>(
          repo.path(),
          localStore,
          stats.copy(),
          edenConfig,
          std::make_unique<SaplingBackingStoreOptions>(
              /*ignoreFilteredPathsConfig=*/false),
          std::make_shared<NullStructuredLogger>(),
          std::make_unique<BackingStoreLogger>(),
          &faultInjector);
};

struct SaplingBackingStoreWithFaultInjectorTest : SaplingBackingStoreTestBase {
  std::shared_ptr<TestConfigSource> testConfigSource{
      std::make_shared<TestConfigSource>(ConfigSourceType::SystemConfig)};
  FaultInjector faultInjector{/*enabled=*/true};

  std::unique_ptr<SaplingBackingStore> queuedBackingStore =
      std::make_unique<SaplingBackingStore>(
          repo.path(),
          localStore,
          stats.copy(),
          edenConfig,
          std::make_unique<SaplingBackingStoreOptions>(
              /*ignoreFilteredPathsConfig=*/false),
          std::make_shared<NullStructuredLogger>(),
          std::make_unique<BackingStoreLogger>(),
          &faultInjector);
};
struct SaplingBackingStoreWithFaultInjectorIgnoreConfigTest
    : SaplingBackingStoreTestBase {
  std::shared_ptr<TestConfigSource> testConfigSource{
      std::make_shared<TestConfigSource>(ConfigSourceType::SystemConfig)};
  FaultInjector faultInjector{/*enabled=*/true};

  std::unique_ptr<SaplingBackingStore> queuedBackingStore =
      std::make_unique<SaplingBackingStore>(
          repo.path(),
          localStore,
          stats.copy(),
          edenConfig,
          std::make_unique<SaplingBackingStoreOptions>(
              /*ignoreFilteredPathsConfig=*/true),
          std::make_shared<NullStructuredLogger>(),
          std::make_unique<BackingStoreLogger>(),
          &faultInjector);
};

} // namespace

TEST_F(SaplingBackingStoreNoFaultInjectorTest, getTree) {
  auto tree1 = queuedBackingStore
                   ->getRootTree(commit1, ObjectFetchContext::getNullContext())
                   .get(kTestTimeout);

  auto [tree2, origin2] =
      queuedBackingStore
          ->getTree(tree1.treeId, ObjectFetchContext::getNullContext())
          .get(kTestTimeout);

  EXPECT_TRUE(*tree1.tree == *tree2);
}

TEST_F(SaplingBackingStoreWithFaultInjectorTest, getTree) {
  auto tree1 = queuedBackingStore
                   ->getRootTree(commit1, ObjectFetchContext::getNullContext())
                   .get(kTestTimeout);

  auto [tree2, origin2] =
      queuedBackingStore
          ->getTree(tree1.treeId, ObjectFetchContext::getNullContext())
          .get(kTestTimeout);

  EXPECT_TRUE(*tree1.tree == *tree2);
}

TEST_F(SaplingBackingStoreNoFaultInjectorTest, getBlob) {
  auto tree = queuedBackingStore
                  ->getRootTree(commit1, ObjectFetchContext::getNullContext())
                  .get(kTestTimeout);

  for (auto& [name, entry] : *tree.tree) {
    if (entry.isTree()) {
      continue;
    }
    if (name == "foo.txt") {
      auto [blob, origin] =
          queuedBackingStore
              ->getBlob(entry.getHash(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "foo\n");
    } else if (name == "bar.txt") {
      auto [blob, origin] =
          queuedBackingStore
              ->getBlob(entry.getHash(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "bar\n");
    }
  }
}

TEST_F(SaplingBackingStoreWithFaultInjectorTest, getBlob) {
  auto tree = queuedBackingStore
                  ->getRootTree(commit1, ObjectFetchContext::getNullContext())
                  .get(kTestTimeout);

  for (auto& [name, entry] : *tree.tree) {
    if (entry.isTree()) {
      continue;
    }
    if (name == "foo.txt") {
      auto [blob, origin] =
          queuedBackingStore
              ->getBlob(entry.getHash(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "foo\n");
    } else if (name == "bar.txt") {
      auto [blob, origin] =
          queuedBackingStore
              ->getBlob(entry.getHash(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "bar\n");
    }
  }
}

TEST_F(SaplingBackingStoreWithFaultInjectorIgnoreConfigTest, getTreeBatch) {
  // force a reload
  updateTestEdenConfig(
      testConfigSource,
      edenConfig,
      {
          {"hg:filtered-paths", "['foo']"},
      });

  auto tree1Hash = HgProxyHash::makeEmbeddedProxyHash1(
      queuedBackingStore->getManifestNode(ObjectId::fromHex(commit1.value()))
          .value(),
      RelativePathPiece{});

  HgProxyHash proxyHash =
      HgProxyHash::load(localStore.get(), tree1Hash, "getTree", *stats);

  auto request = SaplingImportRequest::makeTreeImportRequest(
      tree1Hash,
      proxyHash,
      ObjectFetchContext::getNullContext()->getPriority(),
      ObjectFetchContext::getNullContext()->getCause(),
      ObjectFetchContext::getNullContext()->getClientPid());

  auto executor = std::make_shared<folly::CPUThreadPoolExecutor>(1);
  auto tree1fut = via(executor.get(), [&]() {
    queuedBackingStore->getTreeBatch(
        std::vector{request}, sapling::FetchMode::LocalOnly);
  });

  std::move(tree1fut).get();
  auto tree1 = request->getPromise<TreePtr>()->getFuture().get();

  ASSERT_THAT(
      getTreeNames(tree1),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));
}

TEST_F(SaplingBackingStoreWithFaultInjectorTest, getTreeBatch) {
  {
    updateTestEdenConfig(
        testConfigSource,
        edenConfig,
        {
            {"hg:filtered-paths", "['a/b', 'c/d']"},
        });
  }
  faultInjector.injectBlock("SaplingBackingStore::getTreeBatch", ".*");
  auto tree1Hash = HgProxyHash::makeEmbeddedProxyHash1(
      queuedBackingStore->getManifestNode(ObjectId::fromHex(commit1.value()))
          .value(),
      RelativePathPiece{});

  HgProxyHash proxyHash =
      HgProxyHash::load(localStore.get(), tree1Hash, "getTree", *stats);

  auto request = SaplingImportRequest::makeTreeImportRequest(
      tree1Hash,
      proxyHash,
      ObjectFetchContext::getNullContext()->getPriority(),
      ObjectFetchContext::getNullContext()->getCause(),
      ObjectFetchContext::getNullContext()->getClientPid());

  auto executor = std::make_shared<folly::CPUThreadPoolExecutor>(1);
  auto tree1fut = via(executor.get(), [&]() {
    // this will block until we unblock the fault.
    queuedBackingStore->getTreeBatch(
        std::vector{request}, sapling::FetchMode::LocalOnly);
  });

  // its a bit of a hack, but we need to make sure getTreebatch has hit the
  // fault before we edit the config and unblock it. TODO: We should rewrite
  // SaplingBackingStore with futures so that this is more testable:
  // T171328733.
  /* sleep override */
  sleep(10);

  // force a reload
  updateTestEdenConfig(
      testConfigSource,
      edenConfig,
      {
          {"hg:filtered-paths", "['e/f', 'g/h']"},
      });

  faultInjector.removeFault("SaplingBackingStore::getTreeBatch", ".*");
  ASSERT_EQ(
      faultInjector.unblock("SaplingBackingStore::getTreeBatch", ".*"), 1);

  std::move(tree1fut).get(10s);
  auto tree1 = request->getPromise<TreePtr>()->getFuture().get(10s);

  ASSERT_THAT(
      getTreeNames(tree1),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));
}

TEST(SaplingBackingStoreObjectId, round_trip_object_IDs) {
  Hash20 testHash{
      folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};

  {
    ObjectId legacy{testHash.toByteString()};
    EXPECT_EQ(
        "proxy-0123456789abcdef0123456789abcdef01234567",
        SaplingBackingStore::staticRenderObjectId(legacy));

    EXPECT_EQ(
        legacy,
        SaplingBackingStore::staticParseObjectId(
            SaplingBackingStore::staticRenderObjectId(legacy)));
  }

  {
    ObjectId with_path{HgProxyHash::makeEmbeddedProxyHash1(
        testHash, RelativePathPiece{"foo/bar/baz"})};
    EXPECT_EQ(
        "0123456789abcdef0123456789abcdef01234567:foo/bar/baz",
        SaplingBackingStore::staticRenderObjectId(with_path));

    EXPECT_EQ(
        with_path,
        SaplingBackingStore::staticParseObjectId(
            SaplingBackingStore::staticRenderObjectId(with_path)));
  }

  {
    ObjectId hash_only{HgProxyHash::makeEmbeddedProxyHash2(testHash)};
    EXPECT_EQ(
        "0123456789abcdef0123456789abcdef01234567",
        SaplingBackingStore::staticRenderObjectId(hash_only));

    EXPECT_EQ(
        hash_only,
        SaplingBackingStore::staticParseObjectId(
            SaplingBackingStore::staticRenderObjectId(hash_only)));
  }
}
} // namespace facebook::eden
