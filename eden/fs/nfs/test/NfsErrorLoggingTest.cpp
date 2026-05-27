/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <gtest/gtest.h>
#include <stdexcept>
#include <system_error>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/nfs/Nfsd3.h"
#include "eden/fs/telemetry/ErrorLogger.h"
#include "eden/fs/telemetry/test/CapturingScribeLogger.h"

using namespace facebook::eden;

namespace {

std::shared_ptr<ReloadableConfig> makeTestConfig() {
  auto edenConfig = EdenConfig::createTestEdenConfig();
  edenConfig->enableErrorLogging.setValue(
      true, ConfigSourceType::Default, true);
  return std::make_shared<ReloadableConfig>(edenConfig);
}

} // namespace

TEST(NfsErrorLoggingTest, serverfaultIsLogged) {
  auto scribe = std::make_shared<CapturingScribeLogger>();
  auto config = makeTestConfig();
  ErrorLogger logger(scribe, SessionInfo{}, config);

  folly::exception_wrapper ex{std::runtime_error("backing store failure")};
  detail::logNfsError(
      nfsstat3::NFS3ERR_SERVERFAULT,
      ex,
      logger,
      42,
      canonicalPath("/mnt/repo"));

  ASSERT_EQ(scribe->messages().size(), 1);
  const auto& msg = scribe->messages()[0];
  EXPECT_NE(msg.find("nfs"), std::string::npos)
      << "Should contain nfs component, got: " << msg;
  EXPECT_NE(msg.find("backing store failure"), std::string::npos)
      << "Should contain error message, got: " << msg;
  EXPECT_NE(msg.find("mnt"), std::string::npos)
      << "Should contain mount point, got: " << msg;
}

TEST(NfsErrorLoggingTest, nonServerfaultIsNotLogged) {
  auto scribe = std::make_shared<CapturingScribeLogger>();
  auto config = makeTestConfig();
  ErrorLogger logger(scribe, SessionInfo{}, config);

  folly::exception_wrapper ex{
      std::system_error(ENOENT, std::generic_category(), "file not found")};
  detail::logNfsError(
      nfsstat3::NFS3ERR_NOENT, ex, logger, 42, canonicalPath("/mnt/repo"));

  EXPECT_EQ(scribe->messages().size(), 0)
      << "Non-SERVERFAULT errors should not be logged";
}
