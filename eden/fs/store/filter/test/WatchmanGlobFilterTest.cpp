/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/executors/ManualExecutor.h>
#include <folly/portability/GTest.h>
#include <memory>

#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/store/filter/WatchmanGlobFilter.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeBackingStore.h"

using namespace facebook::eden;
using namespace std::literals::chrono_literals;

class WatchmanGlobFilterTest : public ::testing::Test {
 protected:
  void SetUp() override {
    objectStore_ = makeObjectStore();
  }

  std::shared_ptr<BackingStore> makeFakeBackingStore() {
    auto backingStore = std::make_shared<FakeBackingStore>();
    auto tree1 = backingStore->putTree({
        {"README", backingStore->putBlob("Hello tree1")},
    });
    auto tree2 = backingStore->putTree({
        {"README", backingStore->putBlob("Hello tree2")},
        {"tree1", tree1},
    });
    auto tree3 = backingStore->putTree({
        {"README", backingStore->putBlob("Hello tree3")},
    });
    // root tree
    auto tree = backingStore->putTree({
        {"tree2", tree2}, // /tree2/tree1/README
        {"tree3", tree3}, // /tree3/README
        {"content1.java",
         backingStore->putBlob("meh1"),
         FakeBlobType::REGULAR_FILE},
        {"content2.rs",
         backingStore->putBlob("meh2"),
         FakeBlobType::REGULAR_FILE},
    });
    auto hash = backingStore->putCommit(rootId_, tree);
    hash->setReady();
    tree->setReady();
    tree1->setReady();
    tree2->setReady();
    tree3->setReady();
    return static_cast<std::shared_ptr<BackingStore>>(backingStore);
  }

  std::shared_ptr<ObjectStore> makeObjectStore() {
    backingStore_ = makeFakeBackingStore();
    std::shared_ptr<EdenConfig> rawEdenConfig{
        EdenConfig::createTestEdenConfig()};
    auto edenConfig = std::make_shared<ReloadableConfig>(
        rawEdenConfig, ConfigReloadBehavior::NoReload);
    auto treeCache = TreeCache::create(edenConfig);
    return ObjectStore::create(
        backingStore_,
        treeCache,
        makeRefPtr<EdenStats>(),
        std::make_shared<ProcessInfoCache>(),
        std::make_shared<NullStructuredLogger>(),
        rawEdenConfig,
        true,
        kPathMapDefaultCaseSensitive);
  }

  std::shared_ptr<ObjectStore> objectStore_;
  std::shared_ptr<BackingStore> backingStore_;
  RootId rootId_ = RootId{"1"};
};

TEST_F(WatchmanGlobFilterTest, testGlobbingNotExists) {
  std::vector<std::string> globs = {"tree1/**/*.cpp", "*.cpp", "*.rs"};

  auto executor = folly::ManualExecutor{};
  auto filter = std::make_shared<WatchmanGlobFilter>(
      globs,
      objectStore_,
      ObjectFetchContext::getNullContext(),
      CaseSensitivity::Sensitive);
  auto pass = filter
                  ->isPathFiltered(
                      RelativePathPiece{"tree2/tree1/README"}, rootId_.value())
                  .semi()
                  .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_TRUE(std::move(pass).get());
}

TEST_F(WatchmanGlobFilterTest, testGlobbingExists) {
  std::vector<std::string> globs = {"*.rs"};

  auto executor = folly::ManualExecutor{};
  auto filter = std::make_shared<WatchmanGlobFilter>(
      globs,
      objectStore_,
      ObjectFetchContext::getNullContext(),
      CaseSensitivity::Sensitive);
  auto pass =
      filter->isPathFiltered(RelativePathPiece{"content2.rs"}, rootId_.value())
          .semi()
          .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_FALSE(std::move(pass).get());
}

TEST_F(WatchmanGlobFilterTest, testAnother) {
  std::vector<std::string> globs = {"tree3/README"};

  auto executor = folly::ManualExecutor{};

  auto filter = std::make_shared<WatchmanGlobFilter>(
      globs,
      objectStore_,
      ObjectFetchContext::getNullContext(),
      CaseSensitivity::Sensitive);
  auto pass =
      filter->isPathFiltered(RelativePathPiece{"tree3/README"}, rootId_.value())
          .semi()
          .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_FALSE(std::move(pass).get());
}

TEST_F(WatchmanGlobFilterTest, testGlobs) {
  std::vector<std::string> globs = {"**/README"};

  auto executor = folly::ManualExecutor{};

  auto filter = std::make_shared<WatchmanGlobFilter>(
      globs,
      objectStore_,
      ObjectFetchContext::getNullContext(),
      CaseSensitivity::Sensitive);
  auto pass =
      filter->isPathFiltered(RelativePathPiece{"tree3/README"}, rootId_.value())
          .semi()
          .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_FALSE(std::move(pass).get());
}
