/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/experimental/TestUtil.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GTest.h>
#include <algorithm>
#include <memory>

#include "eden/common/utils/FaultInjector.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/hg/HgBackingStoreOptions.h"
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/HgRepo.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

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
    repo.hg("add");
    commit1 = repo.commit("Initial commit");
    manifest1 = repo.getManifestForCommit(commit1);
  }
};

struct HgQueuedBackingStoreTest : TestRepo, ::testing::Test {
  HgQueuedBackingStoreTest() = default;

  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(EdenConfig::createTestEdenConfig())};
  EdenStatsPtr stats{makeRefPtr<EdenStats>()};
  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>(stats.copy())};

  FaultInjector faultInjector{/*enabled=*/false};

  std::unique_ptr<HgQueuedBackingStore> makeQueuedStore() {
    return std::make_unique<HgQueuedBackingStore>(
        repo.path(),
        localStore,
        stats.copy(),
        edenConfig,
        std::make_shared<NullStructuredLogger>(),
        std::make_unique<BackingStoreLogger>(),
        &faultInjector);
  }
};

} // namespace

TEST_F(HgQueuedBackingStoreTest, getTree) {
  auto queuedStore = makeQueuedStore();
  auto tree1 =
      queuedStore->getRootTree(commit1, ObjectFetchContext::getNullContext())
          .get(kTestTimeout);

  auto [tree2, origin2] =
      queuedStore->getTree(tree1.treeId, ObjectFetchContext::getNullContext())
          .get(kTestTimeout);

  EXPECT_TRUE(*tree1.tree == *tree2);
}

TEST_F(HgQueuedBackingStoreTest, getBlob) {
  auto queuedStore = makeQueuedStore();
  auto tree =
      queuedStore->getRootTree(commit1, ObjectFetchContext::getNullContext())
          .get(kTestTimeout);

  for (auto& [name, entry] : *tree.tree) {
    if (entry.isTree()) {
      continue;
    }
    if (name == "foo.txt") {
      auto [blob, origin] =
          queuedStore
              ->getBlob(entry.getHash(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "foo\n");
    } else if (name == "bar.txt") {
      auto [blob, origin] =
          queuedStore
              ->getBlob(entry.getHash(), ObjectFetchContext::getNullContext())
              .get(kTestTimeout);

      EXPECT_EQ(blob->getContents().cloneAsValue().moveToFbString(), "bar\n");
    }
  }
}

TEST(HgQueuedBackingStoreObjectId, round_trip_object_IDs) {
  Hash20 testHash{
      folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};

  {
    ObjectId legacy{testHash.toByteString()};
    EXPECT_EQ(
        "proxy-0123456789abcdef0123456789abcdef01234567",
        HgQueuedBackingStore::staticRenderObjectId(legacy));

    EXPECT_EQ(
        legacy,
        HgQueuedBackingStore::staticParseObjectId(
            HgQueuedBackingStore::staticRenderObjectId(legacy)));
  }

  {
    ObjectId with_path{HgProxyHash::makeEmbeddedProxyHash1(
        testHash, RelativePathPiece{"foo/bar/baz"})};
    EXPECT_EQ(
        "0123456789abcdef0123456789abcdef01234567:foo/bar/baz",
        HgQueuedBackingStore::staticRenderObjectId(with_path));

    EXPECT_EQ(
        with_path,
        HgQueuedBackingStore::staticParseObjectId(
            HgQueuedBackingStore::staticRenderObjectId(with_path)));
  }

  {
    ObjectId hash_only{HgProxyHash::makeEmbeddedProxyHash2(testHash)};
    EXPECT_EQ(
        "0123456789abcdef0123456789abcdef01234567",
        HgQueuedBackingStore::staticRenderObjectId(hash_only));

    EXPECT_EQ(
        hash_only,
        HgQueuedBackingStore::staticParseObjectId(
            HgQueuedBackingStore::staticRenderObjectId(hash_only)));
  }
}