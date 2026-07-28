/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GTest.h>
#include <folly/synchronization/SaturatingSemaphore.h>

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeAccessLogger.h"
#include "eden/fs/inodes/TreeInode.h" // @nolint
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/XplatKeys.h"
#include "eden/fs/telemetry/facebook/EdenTelemetryIdentity.h"
#include "eden/fs/telemetry/facebook/XplatLogger.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;

namespace {

std::shared_ptr<ReloadableConfig> makeTestReloadableConfig() {
  return std::make_shared<ReloadableConfig>(EdenConfig::createTestEdenConfig());
}

class SpyXplatLogger : public XplatLogger {
 public:
  SpyXplatLogger()
      : XplatLogger(
            EdenTelemetryIdentity{},
            makeRefPtr<EdenStats>(),
            makeTestReloadableConfig()) {}

  std::atomic<int> callCount{0};
  folly::SaturatingSemaphore<> eventLogged;
  std::string lastCategory;
  DynamicEvent lastEvent;

  void logEvent(std::string_view category, const DynamicEvent& event)
      override {
    lastCategory = std::string{category};
    lastEvent = event;
    callCount.fetch_add(1);
    eventLogged.post();
  }
};

class InodeAccessLoggerXplatTest : public ::testing::Test {
 protected:
  void SetUp() override {
    builder_.setFile("src/main.cpp", "int main() {}\n");
    testMount_ = std::make_unique<TestMount>(builder_);
    testMount_->getBackingStore()->setRepoName("test_repo");
  }

  InodeAccess makeTestEvent() {
    auto fileInode = testMount_->getFileInode("src/main.cpp");
    return InodeAccess{
        fileInode->getNodeId(),
        dtype_t::Regular,
        ObjectFetchContext::Cause::Fs,
        std::nullopt,
        testMount_->getEdenMount()};
  }

  std::unique_ptr<InodeAccessLogger> createLogger(
      std::shared_ptr<IXplatLogger> xplatLogger) {
    auto config = EdenConfig::createTestEdenConfig();
    config->logFileAccesses.setValue(true, ConfigSourceType::UserConfig, true);
    config->logFileAccessesSamplingDenominator.setValue(
        1, ConfigSourceType::UserConfig, true);

    auto reloadableConfig =
        std::make_shared<ReloadableConfig>(std::move(config));
    return std::make_unique<InodeAccessLogger>(
        std::move(reloadableConfig),
        makeRefPtr<EdenStats>(),
        std::move(xplatLogger));
  }

  FakeTreeBuilder builder_;
  std::unique_ptr<TestMount> testMount_;
};

TEST_F(InodeAccessLoggerXplatTest, logsFileAccessViaXplat) {
  auto spyXplatLogger = std::make_shared<SpyXplatLogger>();
  std::weak_ptr<SpyXplatLogger> weakXplatLogger = spyXplatLogger;
  auto logger = createLogger(spyXplatLogger);
  spyXplatLogger.reset();

  EXPECT_FALSE(weakXplatLogger.expired());
  auto retainedXplatLogger = weakXplatLogger.lock();
  ASSERT_NE(nullptr, retainedXplatLogger);

  logger->logInodeAccess(makeTestEvent());
  ASSERT_TRUE(
      retainedXplatLogger->eventLogged.try_wait_for(std::chrono::seconds(5)));
  logger.reset();

  EXPECT_EQ(1, retainedXplatLogger->callCount.load());
  EXPECT_EQ(
      xplat_keys::kFileAccessCategory, retainedXplatLogger->lastCategory);
  const auto& strings = retainedXplatLogger->lastEvent.getStringMap();
  EXPECT_EQ("test_repo", strings.at(std::string{xplat_keys::kRepo}));
  EXPECT_EQ("src", strings.at(std::string{xplat_keys::kDirectory}));
  EXPECT_EQ("main.cpp", strings.at(std::string{xplat_keys::kFilename}));
  EXPECT_EQ("fs", strings.at(std::string{xplat_keys::kSource}));

  retainedXplatLogger.reset();
  EXPECT_TRUE(weakXplatLogger.expired());
}

TEST_F(InodeAccessLoggerXplatTest, nullXplatLoggerNoOps) {
  // OSS builds have no xplat logger. The worker thread must no-op on the
  // null-logger guard rather than dereferencing a removed backend; this test
  // regresses the guard by draining a queued access with no logger attached.
  auto logger = createLogger(nullptr);

  logger->logInodeAccess(makeTestEvent());
  // The destructor joins the worker thread, which drains the queued event
  // through the null guard. Without the guard this would crash.
  logger.reset();
}

} // namespace
