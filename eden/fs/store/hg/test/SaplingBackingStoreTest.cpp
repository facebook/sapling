/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/logging/xlog.h>
#include <folly/testing/TestUtil.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
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
    repo.hgInit(testPath + "cache"_pc, {}, /* isEagerRepo */ true);

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
  std::shared_ptr<EdenConfig> testEdenConfig =
      EdenConfig::createTestEdenConfig();
  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(testEdenConfig)};
  EdenStatsPtr stats{makeRefPtr<EdenStats>()};
  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>(stats.copy())};
};

struct SaplingBackingStoreNoFaultInjectorTest : SaplingBackingStoreTestBase {
  FaultInjector faultInjector{/*enabled=*/false};
  folly::InlineExecutor executor = folly::InlineExecutor::instance();

  std::unique_ptr<SaplingBackingStore> queuedBackingStore =
      std::make_unique<SaplingBackingStore>(
          repo.path(),
          repo.path(),
          localStore,
          stats.copy(),
          &executor,
          edenConfig,
          std::make_unique<SaplingBackingStoreOptions>(),
          std::make_shared<NullStructuredLogger>(),
          std::make_unique<BackingStoreLogger>(),
          &faultInjector);
};

struct SaplingBackingStoreWithFaultInjectorTest : SaplingBackingStoreTestBase {
  std::shared_ptr<TestConfigSource> testConfigSource{
      std::make_shared<TestConfigSource>(ConfigSourceType::SystemConfig)};
  FaultInjector faultInjector{/*enabled=*/true};
  folly::InlineExecutor executor = folly::InlineExecutor::instance();

  std::unique_ptr<SaplingBackingStore> queuedBackingStore =
      std::make_unique<SaplingBackingStore>(
          repo.path(),
          repo.path(),
          localStore,
          stats.copy(),
          &executor,
          edenConfig,
          std::make_unique<SaplingBackingStoreOptions>(),
          std::make_shared<NullStructuredLogger>(),
          std::make_unique<BackingStoreLogger>(),
          &faultInjector);
};

struct SaplingBackingStoreWithFaultInjectorIgnoreConfigTest
    : SaplingBackingStoreTestBase {
  std::shared_ptr<TestConfigSource> testConfigSource{
      std::make_shared<TestConfigSource>(ConfigSourceType::SystemConfig)};
  FaultInjector faultInjector{/*enabled=*/true};
  folly::InlineExecutor executor = folly::InlineExecutor::instance();

  std::unique_ptr<SaplingBackingStore> queuedBackingStore =
      std::make_unique<SaplingBackingStore>(
          repo.path(),
          repo.path(),
          localStore,
          stats.copy(),
          &executor,
          edenConfig,
          std::make_unique<SaplingBackingStoreOptions>(),
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
              ->getBlob(
                  entry.getObjectId(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "foo\n");
    } else if (name == "bar.txt") {
      auto [blob, origin] =
          queuedBackingStore
              ->getBlob(
                  entry.getObjectId(), ObjectFetchContext::getNullContext())
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
              ->getBlob(
                  entry.getObjectId(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "foo\n");
    } else if (name == "bar.txt") {
      auto [blob, origin] =
          queuedBackingStore
              ->getBlob(
                  entry.getObjectId(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "bar\n");
    }
  }
}

TEST_F(SaplingBackingStoreNoFaultInjectorTest, getGlobFilesMultiple) {
  auto suffixes = std::vector<std::string>{".txt"};
  auto prefixes = std::vector<std::string>{};
  auto globFiles = queuedBackingStore->getGlobFiles(commit1, suffixes, prefixes)
                       .get(kTestTimeout);
  auto paths = globFiles.globFiles;
  auto commitId = queuedBackingStore->renderRootId(globFiles.rootId);

  EXPECT_EQ(commitId, queuedBackingStore->renderRootId(commit1));

  // TODO(T189729875) Make it check the files created during setup
  // The globFiles SaplingRemoteAPI endpoint is currently mocked out so files
  // returned are always the same dependent on the given suffix.
  std::sort(paths.begin(), paths.end());
  auto expected_result = std::vector<std::string>{"baz.txt", "foo.txt"};
  EXPECT_EQ(paths.size(), 2);
  for (int i = 0; i < 2; i++) {
    EXPECT_EQ(paths[i], expected_result[i]);
  }
}

TEST_F(SaplingBackingStoreNoFaultInjectorTest, getGlobFilesSingle) {
  auto suffixes = std::vector<std::string>{".rs"};
  auto prefixes = std::vector<std::string>{};
  auto globFiles = queuedBackingStore->getGlobFiles(commit1, suffixes, prefixes)
                       .get(kTestTimeout);
  auto paths = globFiles.globFiles;
  auto commitId = queuedBackingStore->renderRootId(globFiles.rootId);

  EXPECT_EQ(commitId, queuedBackingStore->renderRootId(commit1));

  // TODO(T189729875) Make it check the files created during setup
  // The globFiles SaplingRemoteAPI endpoint is currently mocked out so files
  // returned are always the same dependent on the given suffix.
  std::sort(paths.begin(), paths.end());
  auto expected_result = std::vector<std::string>{"bar.rs"};
  EXPECT_EQ(paths.size(), 1);
  EXPECT_EQ(paths[0], expected_result[0]);
}
TEST_F(SaplingBackingStoreNoFaultInjectorTest, getGlobFilesNone) {
  auto suffixes = std::vector<std::string>{".bzl"};
  auto prefixes = std::vector<std::string>{};
  auto globFiles = queuedBackingStore->getGlobFiles(commit1, suffixes, prefixes)
                       .get(kTestTimeout);
  auto paths = globFiles.globFiles;
  auto commitId = queuedBackingStore->renderRootId(globFiles.rootId);

  EXPECT_EQ(commitId, queuedBackingStore->renderRootId(commit1));

  // TODO(T189729875) Make it check the files created during setup
  // The globFiles SaplingRemoteAPI endpoint is currently mocked out so files
  // returned are always the same dependent on the given suffix.
  EXPECT_EQ(paths.size(), 0);
}

TEST_F(SaplingBackingStoreNoFaultInjectorTest, getGlobFilesNested) {
  auto suffixes = std::vector<std::string>{".cpp"};
  auto prefixes = std::vector<std::string>{};
  auto globFiles = queuedBackingStore->getGlobFiles(commit1, suffixes, prefixes)
                       .get(kTestTimeout);
  auto paths = globFiles.globFiles;
  auto commitId = queuedBackingStore->renderRootId(globFiles.rootId);

  EXPECT_EQ(commitId, queuedBackingStore->renderRootId(commit1));

  // TODO(T189729875) Make it check the files created during setup
  // The globFiles SaplingRemoteAPI endpoint is currently mocked out so files
  // returned are always the same dependent on the given suffix.
  std::sort(paths.begin(), paths.end());
  auto expected_result =
      std::vector<std::string>{"fuji/peak.cpp", "ranier.cpp"};
  EXPECT_EQ(paths.size(), 2);
  for (int i = 0; i < 2; i++) {
    EXPECT_EQ(paths[i], expected_result[i]);
  }
}

TEST_F(SaplingBackingStoreNoFaultInjectorTest, cachingPolicyConstruction) {
  // No caching
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::NoCaching);

  // Trees
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::Trees);

  // TreeAuxData
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::TreeAuxData);

  // Blobs
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::Blobs);

  // BlobAuxData
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::BlobAuxData);

  // TreesAndBlobs
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobs);

  // TreesAndBlobAuxData
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobAuxData);

  // BlobsAndBlobAuxData
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      false, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::BlobsAndBlobAuxData);

  // Anything
  testEdenConfig->hgEnableTreeLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableTreeMetaLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  testEdenConfig->hgEnableBlobMetaLocalStoreCaching.setValue(
      true, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      queuedBackingStore->constructLocalStoreCachingPolicy(),
      BackingStore::LocalStoreCachingPolicy::Anything);
}

// Duplicate requests with same nodeID in one request batch will crash Eden with
// `folly::PromiseAlreadySatisfied` exception. It caused S463588 in the past.
// More info on the investication doc
// https://fburl.com/gdoc/7mgp3fhe
// This test is to make sure we don't have duplicate requests in one batch.
TEST_F(
    SaplingBackingStoreNoFaultInjectorTest,
    sameRequestsDifferentFetchCause) {
  auto treeId = HgProxyHash::makeEmbeddedProxyHash1(
      queuedBackingStore->getManifestNode(ObjectId::fromHex(commit1.value()))
          .value(),
      RelativePathPiece{});

  HgProxyHash proxyHash =
      HgProxyHash::load(localStore.get(), treeId, "getTree", *stats);

  auto fsRequestContext = ObjectFetchContext::getNullFsContext();
  auto prefetchRequestContext = ObjectFetchContext::getNullPrefetchContext();
  auto fsRequest = SaplingImportRequest::makeTreeImportRequest(
      treeId, proxyHash, fsRequestContext);
  auto prefetchRequest = SaplingImportRequest::makeTreeImportRequest(
      treeId, proxyHash, prefetchRequestContext);

  auto executor = std::make_shared<folly::CPUThreadPoolExecutor>(1);
  auto treeFuture = via(executor.get(), [&]() {
    queuedBackingStore->getTreeBatch(
        std::vector{fsRequest, prefetchRequest}, sapling::FetchMode::LocalOnly);
  });

  std::move(treeFuture).get();
  auto tree1 = fsRequest->getPromise<TreePtr>()->getFuture().get();

  ASSERT_THAT(
      getTreeNames(tree1),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));
}

TEST_F(SaplingBackingStoreWithFaultInjectorIgnoreConfigTest, getTreeBatch) {
  // force a reload
  updateTestEdenConfig(
      testConfigSource,
      edenConfig,
      {
          {"hg:filtered-paths", "['foo']"},
      });

  auto tree1Id = HgProxyHash::makeEmbeddedProxyHash1(
      queuedBackingStore->getManifestNode(ObjectId::fromHex(commit1.value()))
          .value(),
      RelativePathPiece{});

  HgProxyHash proxyHash =
      HgProxyHash::load(localStore.get(), tree1Id, "getTree", *stats);

  auto requestContext = ObjectFetchContext::getNullContext();
  auto request = SaplingImportRequest::makeTreeImportRequest(
      tree1Id, proxyHash, requestContext);

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
  auto tree1Id = HgProxyHash::makeEmbeddedProxyHash1(
      queuedBackingStore->getManifestNode(ObjectId::fromHex(commit1.value()))
          .value(),
      RelativePathPiece{});

  HgProxyHash proxyHash =
      HgProxyHash::load(localStore.get(), tree1Id, "getTree", *stats);

  auto requestContext = ObjectFetchContext::getNullContext();
  auto request = SaplingImportRequest::makeTreeImportRequest(
      tree1Id, proxyHash, requestContext);

  auto executor = std::make_shared<folly::CPUThreadPoolExecutor>(1);
  auto tree1fut = via(executor.get(), [&]() {
    // this will block until we unblock the fault.
    queuedBackingStore->getTreeBatch(
        std::vector{request}, sapling::FetchMode::LocalOnly);
  });

  // TODO: We should rewrite SaplingBackingStore with futures so that this is
  // more testable: T171328733.
  ASSERT_TRUE(
      faultInjector.waitUntilBlocked("SaplingBackingStore::getTreeBatch", 10s));

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
  Hash20 testId{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};

  {
    ObjectId legacy{testId.toByteString()};
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
        testId, RelativePathPiece{"foo/bar/baz"})};
    EXPECT_EQ(
        "0123456789abcdef0123456789abcdef01234567:foo/bar/baz",
        SaplingBackingStore::staticRenderObjectId(with_path));

    EXPECT_EQ(
        with_path,
        SaplingBackingStore::staticParseObjectId(
            SaplingBackingStore::staticRenderObjectId(with_path)));
  }

  {
    ObjectId id_only{HgProxyHash::makeEmbeddedProxyHash2(testId)};
    EXPECT_EQ(
        "0123456789abcdef0123456789abcdef01234567",
        SaplingBackingStore::staticRenderObjectId(id_only));

    EXPECT_EQ(
        id_only,
        SaplingBackingStore::staticParseObjectId(
            SaplingBackingStore::staticRenderObjectId(id_only)));
  }
}
} // namespace facebook::eden
