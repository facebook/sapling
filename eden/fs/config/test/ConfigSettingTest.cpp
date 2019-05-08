/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/config/EdenConfig.h"

#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/utils/PathFuncs.h"

using folly::StringPiece;

using namespace facebook::eden;
using namespace folly::string_piece_literals;

TEST(ConfigSettingTest, initStateCheck) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  // Initial should be default
  EXPECT_EQ(testDir.getValue(), defaultDir);
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getConfigKey(), dirKey);
}

TEST(ConfigSettingTest, configSetStringValue) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  folly::StringPiece systemConfigDir{"/SYSTEM_CONFIG_SETTING"};
  std::map<std::string, std::string> attrMap;
  auto rslt = testDir.setStringValue(
      systemConfigDir, attrMap, ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), systemConfigDir);

  folly::StringPiece userConfigDir{"/USER_CONFIG_SETTING"};
  rslt = testDir.setStringValue(
      userConfigDir, attrMap, ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), userConfigDir);
}

TEST(ConfigSettingTest, configSetAssign) {
  // Setup our target copy
  AbsolutePath otherDir{"/OTHER_DIR"};
  auto otherKey = "otherKey"_sp;
  ConfigSetting<AbsolutePath> copyOfTestDir{otherKey, otherDir, nullptr};
  folly::StringPiece systemConfigDir{"/SYSTEM_CONFIG_SETTING"};

  // Check the copy states first, so we know where starting point is.
  EXPECT_EQ(copyOfTestDir.getConfigKey(), otherKey);
  EXPECT_EQ(copyOfTestDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(copyOfTestDir.getValue(), otherDir);

  auto dirKey = "dirKey"_sp;
  {
    // Setup the copy source - sufficiently different
    AbsolutePath defaultDir{"/DEFAULT_DIR"};
    ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

    std::map<std::string, std::string> attrMap;
    auto rslt = testDir.setStringValue(
        systemConfigDir, attrMap, ConfigSource::USER_CONFIG_FILE);
    EXPECT_EQ(rslt.hasError(), false);

    EXPECT_EQ(testDir.getConfigKey(), dirKey);
    EXPECT_EQ(testDir.getSource(), ConfigSource::USER_CONFIG_FILE);
    EXPECT_EQ(testDir.getValue(), systemConfigDir);

    copyOfTestDir.copyFrom(testDir);
  }

  // Check all attributes copied.
  EXPECT_EQ(copyOfTestDir.getConfigKey(), dirKey);
  EXPECT_EQ(copyOfTestDir.getSource(), ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(copyOfTestDir.getValue(), systemConfigDir);

  // Check references still valid
  copyOfTestDir.clearValue(ConfigSource::DEFAULT);
}

TEST(ConfigSettingTest, configSetInvalidStringValue) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  folly::StringPiece systemConfigDir{"/SYSTEM_CONFIG_SETTING"};
  std::map<std::string, std::string> attrMap;
  auto rslt = testDir.setStringValue(
      systemConfigDir, attrMap, ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), systemConfigDir);

  folly::StringPiece userConfigDir{"INVALID USER_CONFIG_SETTING"};
  rslt = testDir.setStringValue(
      userConfigDir, attrMap, ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(rslt.hasError(), true);
  EXPECT_EQ(
      rslt.error(),
      "Cannot convert value 'INVALID USER_CONFIG_SETTING' to an absolute path");
  EXPECT_EQ(testDir.getSource(), ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), systemConfigDir);
}

TEST(ConfigSettingTest, configSetEnvSubTest) {
  AbsolutePath defaultDir{"/home/bob"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  folly::StringPiece userConfigDir{"${HOME}/test_dir"};
  std::map<std::string, std::string> attrMap;
  attrMap["HOME"] = "/home/bob";
  attrMap["USER"] = "bob";
  auto rslt = testDir.setStringValue(
      userConfigDir, attrMap, ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), "/home/bob/test_dir");

  folly::StringPiece homeUserConfigDir{"/home/${USER}/test_dir"};
  rslt = testDir.setStringValue(
      homeUserConfigDir, attrMap, ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), "/home/bob/test_dir");
}

TEST(ConfigSettingTest, configSettingIgnoreDefault) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};
  // Initial should be default
  EXPECT_EQ(testDir.getValue(), defaultDir);
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);

  // Setting default value should be ignored
  AbsolutePath notDefaultDir{"/NOT_THE_DEFAULT_DIR"};
  testDir.setValue(notDefaultDir, ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getValue(), defaultDir);

  // Clearing the default value should be ignored
  testDir.clearValue(ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getValue(), defaultDir);
}

TEST(ConfigSettingTest, configSettingClearNonExistingSource) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);

  // Clear unset priorities
  testDir.clearValue(ConfigSource::COMMAND_LINE);
  testDir.clearValue(ConfigSource::USER_CONFIG_FILE);
  testDir.clearValue(ConfigSource::SYSTEM_CONFIG_FILE);
  testDir.clearValue(ConfigSource::DEFAULT);

  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getValue(), defaultDir);
}

TEST(ConfigSettingTest, configSettingSetAndClearTest) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  AbsolutePath systemEdenDir{"/SYSTEM_DIR"};

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getValue(), defaultDir);

  // Over-ride default
  testDir.setValue(systemEdenDir, ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), systemEdenDir);

  // Clear the over-ride
  testDir.clearValue(ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getValue(), defaultDir);
}

TEST(ConfigSettingTest, configSetOverRiddenSource) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  AbsolutePath cliEdenDir{"/CLI_DIR"};
  AbsolutePath systemEdenDir{"/SYSTEM_DIR"};

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);

  // Set the highest priority item
  testDir.setValue(cliEdenDir, ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Set a middle priority item (results same as above)
  testDir.setValue(systemEdenDir, ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear current highest priority
  testDir.clearValue(ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), systemEdenDir);
}

TEST(ConfigSettingTest, configClearOverRiddenSource) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  AbsolutePath cliEdenDir{"/CLI_DIR"};
  AbsolutePath userEdenDir{"/USER_DIR"};
  AbsolutePath systemEdenDir{"/SYSTEM_DIR"};

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getValue(), defaultDir);

  // Set next higher over-ride priority
  testDir.setValue(systemEdenDir, ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), systemEdenDir);

  // Set next higher over-ride priority
  testDir.setValue(userEdenDir, ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(testDir.getValue(), userEdenDir);

  // Set next higher over-ride priority
  testDir.setValue(cliEdenDir, ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear the middle priority item (no effect on source/value)
  testDir.clearValue(ConfigSource::USER_CONFIG_FILE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear the middle priority item (no effect on source/value)
  testDir.clearValue(ConfigSource::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear highest priority - back to default
  testDir.clearValue(ConfigSource::COMMAND_LINE);
  EXPECT_EQ(testDir.getSource(), ConfigSource::DEFAULT);
  EXPECT_EQ(testDir.getValue(), defaultDir);
}
