/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/experimental/TestUtil.h>
#include <folly/portability/GMock.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgDatapackStore.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/testharness/HgRepo.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/testharness/TestConfigSource.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/ImmediateFuture.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

struct TestRepo {
  folly::test::TemporaryDirectory testDir{"eden_hg_datapack_store_test"};
  AbsolutePath testPath = canonicalPath(testDir.path().string());
  HgRepo repo{testPath + "repo"_pc};
  RootId commit1;

  TestRepo() {
    repo.hgInit(testPath + "cache"_pc);

    repo.mkdir("foo");
    repo.writeFile("foo/bar.txt", "bar\n");
    repo.mkdir("src");
    repo.writeFile("src/hello.txt", "world\n");
    repo.hg("add", "foo", "src");
    commit1 = repo.commit("Initial commit");
  }
};

HgDatapackStore::Options testOptions() {
  HgDatapackStore::Options options{};
  options.allow_retries = false;
  return options;
}

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

struct HgDatapackStoreTest : TestRepo, ::testing::Test {
  EdenStatsPtr stats{makeRefPtr<EdenStats>()};

  HgDatapackStore::Options options{testOptions()};

  std::shared_ptr<TestConfigSource> testConfigSource{
      std::make_shared<TestConfigSource>(ConfigSourceType::SystemConfig)};

  std::unique_ptr<folly::test::TemporaryDirectory> testDir =
      std::make_unique<folly::test::TemporaryDirectory>(makeTempDir());

  std::shared_ptr<EdenConfig> rawEdenConfig{std::make_shared<EdenConfig>(
      ConfigVariables{},
      /*userHomePath=*/canonicalPath(testDir->path().string()),
      /*systemConfigDir=*/canonicalPath(testDir->path().string()),
      EdenConfig::SourceVector{testConfigSource})};

  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(rawEdenConfig)};
  FaultInjector faultInjector{/*enabled=*/true};

  HgDatapackStore datapackStore{
      repo.path(),
      options,
      edenConfig,
      nullptr,
      &faultInjector};
  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>(stats.copy())};
};

TEST_F(HgDatapackStoreTest, getTreeBatch) {
  {
    updateTestEdenConfig(
        testConfigSource,
        edenConfig,
        {
            {"hg:filtered-paths", "['a/b', 'c/d']"},
        });
  }
  faultInjector.injectBlock("HgDatapackStore::getTreeBatch", ".*");
  auto tree1Hash = HgProxyHash::makeEmbeddedProxyHash1(
      datapackStore.getManifestNode(ObjectId::fromHex(commit1.value())).value(),
      RelativePathPiece{});

  HgProxyHash proxyHash =
      HgProxyHash::load(localStore.get(), tree1Hash, "getTree", *stats);

  auto request = HgImportRequest::makeTreeImportRequest(
      tree1Hash,
      proxyHash,
      ObjectFetchContext::getNullContext()->getPriority(),
      ObjectFetchContext::getNullContext()->getCause(),
      ObjectFetchContext::getNullContext()->getClientPid());

  auto executor = std::make_shared<folly::CPUThreadPoolExecutor>(1);
  auto tree1fut = via(executor.get(), [&]() {
    // this will block until we unblock the fault.
    this->datapackStore.getTreeBatch(std::vector{request});
  });

  // its a bit of a hack, but we need to make sure getTreebatch has hit the
  // fault before we edit the config and unblock it. TODO: We should rewrite
  // HgDatapackStore with futures so that this is more testable: T171328733.
  /* sleep override */
  sleep(10);

  // force a reload
  updateTestEdenConfig(
      testConfigSource,
      edenConfig,
      {
          {"hg:filtered-paths", "['e/f', 'g/h']"},
      });

  faultInjector.removeFault("HgDatapackStore::getTreeBatch", ".*");
  ASSERT_EQ(faultInjector.unblock("HgDatapackStore::getTreeBatch", ".*"), 1);

  std::move(tree1fut).get(10s);
  auto tree1 = request->getPromise<TreePtr>()->getFuture().get(10s);

  ASSERT_THAT(
      getTreeNames(tree1),
      ::testing::ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));
}
