/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/EdenConfig.h"

#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/TomlFileConfigSource.h"

using folly::test::TemporaryDirectory;
using namespace facebook::eden;
using namespace facebook::eden::path_literals;

namespace {

ConfigVariables testVariables() {
  ConfigVariables variables;
  variables["HOME"] = "/home/testuser";
  variables["USER"] = "testuser";
  variables["USER_ID"] = "0";
  return variables;
}

} // namespace

TEST(EdenConfigReloadTest, systemConfigDirAfterReload) {
  TemporaryDirectory tmp{"eden_config_reload_test_"};
  auto root = canonicalPath(tmp.path().string());
  auto etcEdenDir = root + "etc-eden"_pc;
  ensureDirectoryExists(etcEdenDir);
  auto dynamicConfigPath = etcEdenDir + "edenfs_dynamic.rc"_pc;
  writeFile(
      dynamicConfigPath,
      folly::StringPiece{"[treecache]\nminimum-items = \"32\"\n"})
      .value();

  auto config = std::make_shared<EdenConfig>(
      testVariables(),
      canonicalPath("/home/testuser"),
      etcEdenDir,
      EdenConfig::SourceVector{std::make_shared<TomlFileConfigSource>(
          dynamicConfigPath, ConfigSourceType::Dynamic)});
  EXPECT_EQ(etcEdenDir, config->getSystemConfigDir());
  EXPECT_EQ(32, config->inMemoryTreeCacheMinimumItems.getValue());

  writeFile(
      dynamicConfigPath,
      folly::StringPiece{"[treecache]\nminimum-items = \"64\"\n\n"})
      .value();
  auto reloaded = config->maybeReload();
  ASSERT_NE(nullptr, reloaded);
  EXPECT_EQ(64, reloaded->inMemoryTreeCacheMinimumItems.getValue());
  EXPECT_EQ(AbsolutePath{}, reloaded->getSystemConfigDir());
}
