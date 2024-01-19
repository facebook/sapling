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
#include "eden/fs/store/filter/GlobFilter.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeBackingStore.h"

using namespace facebook::eden;
using namespace std::literals::chrono_literals;

TEST(GlobFilterTest, testGlobbingNotExists) {
  std::vector<std::string> globs = {"tree1/**/*.cpp", "*.cpp", "*.rs"};

  auto executor = folly::ManualExecutor{};
  auto filter = std::make_shared<GlobFilter>(globs, CaseSensitivity::Sensitive);
  auto pass =
      filter
          ->getFilterCoverageForPath(
              RelativePathPiece{"tree2/tree1/README"}, folly::StringPiece(""))
          .semi()
          .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_EQ(std::move(pass).get(0ms), FilterCoverage::RECURSIVELY_FILTERED);
}

TEST(GlobFilterTest, testGlobbingExists) {
  std::vector<std::string> globs = {"*.rs"};

  auto executor = folly::ManualExecutor{};
  auto filter = std::make_shared<GlobFilter>(globs, CaseSensitivity::Sensitive);
  auto pass = filter
                  ->getFilterCoverageForPath(
                      RelativePathPiece{"content2.rs"}, folly::StringPiece(""))
                  .semi()
                  .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_EQ(std::move(pass).get(0ms), FilterCoverage::UNFILTERED);
}

TEST(GlobFilterTest, testAnother) {
  std::vector<std::string> globs = {"tree3/README"};

  auto executor = folly::ManualExecutor{};

  auto filter = std::make_shared<GlobFilter>(globs, CaseSensitivity::Sensitive);
  auto pass = filter
                  ->getFilterCoverageForPath(
                      RelativePathPiece{"tree3/README"}, folly::StringPiece(""))
                  .semi()
                  .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_EQ(std::move(pass).get(0ms), FilterCoverage::UNFILTERED);
}

TEST(GlobFilterTest, testGlobs) {
  std::vector<std::string> globs = {"**/README"};

  auto executor = folly::ManualExecutor{};

  auto filter = std::make_shared<GlobFilter>(globs, CaseSensitivity::Sensitive);
  auto pass = filter
                  ->getFilterCoverageForPath(
                      RelativePathPiece{"tree3/README"}, folly::StringPiece(""))
                  .semi()
                  .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_EQ(std::move(pass).get(0ms), FilterCoverage::UNFILTERED);
}

TEST(GlobFilterTest, testComplexGlobs) {
  std::vector<std::string> globs = {"a/b/**/README"};

  auto executor = folly::ManualExecutor{};

  auto filter = std::make_shared<GlobFilter>(globs, CaseSensitivity::Sensitive);
  auto pass = filter
                  ->getFilterCoverageForPath(
                      RelativePathPiece{"a/b/c"}, folly::StringPiece(""))
                  .semi()
                  .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_EQ(std::move(pass).get(), FilterCoverage::UNFILTERED);

  auto notPass = filter
                     ->getFilterCoverageForPath(
                         RelativePathPiece{"a/c/b.cpp"}, folly::StringPiece(""))
                     .semi()
                     .via(folly::Executor::getKeepAliveToken(executor));

  executor.drain();

  EXPECT_EQ(std::move(notPass).get(), FilterCoverage::RECURSIVELY_FILTERED);
}

TEST(GlobFilterTest, testFiltered) {
  std::vector<std::string> globs = {"a/b/**/README"};

  auto executor = folly::ManualExecutor{};

  auto filter = std::make_shared<GlobFilter>(globs, CaseSensitivity::Sensitive);
  auto pass = filter
                  ->getFilterCoverageForPath(
                      RelativePathPiece{"a/d/c"}, folly::StringPiece(""))
                  .semi()
                  .via(folly::Executor::getKeepAliveToken(executor));
  executor.drain();

  EXPECT_EQ(std::move(pass).get(), FilterCoverage::RECURSIVELY_FILTERED);
}
