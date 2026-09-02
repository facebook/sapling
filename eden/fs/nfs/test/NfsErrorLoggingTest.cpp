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
#include "eden/fs/telemetry/test/CapturingXplatLogger.h"

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
  CapturingXplatLogger xplatLogger;
  auto config = makeTestConfig();
  ErrorLogger logger(config, &xplatLogger);
  const auto mountPath = canonicalPath("/mnt/repo");

  folly::exception_wrapper ex{std::runtime_error("backing store failure")};
  detail::logNfsError(nfsstat3::NFS3ERR_SERVERFAULT, ex, logger, 42, mountPath);

  ASSERT_EQ(xplatLogger.events().size(), 1);
  const auto& strings = xplatLogger.events()[0].event.getStringMap();
  EXPECT_EQ(strings.at("component"), "nfs");
  EXPECT_EQ(strings.at("error_message"), "backing store failure");
  EXPECT_EQ(strings.at("mount_point"), mountPath.value());
}

TEST(NfsErrorLoggingTest, nonServerfaultIsNotLogged) {
  CapturingXplatLogger xplatLogger;
  auto config = makeTestConfig();
  ErrorLogger logger(config, &xplatLogger);

  folly::exception_wrapper ex{
      std::system_error(ENOENT, std::generic_category(), "file not found")};
  detail::logNfsError(
      nfsstat3::NFS3ERR_NOENT, ex, logger, 42, canonicalPath("/mnt/repo"));

  EXPECT_EQ(xplatLogger.events().size(), 0)
      << "Non-SERVERFAULT errors should not be logged";
}
