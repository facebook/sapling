/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/EdenConfig.h"

#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/utils/PathFuncs.h"

using folly::StringPiece;

using namespace facebook::eden;
using namespace folly::string_piece_literals;
using namespace std::chrono_literals;

TEST(ConfigSettingTest, initStateCheck) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  // Initial should be default
  EXPECT_EQ(testDir.getValue(), defaultDir);
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(testDir.getConfigKey(), dirKey);
}

TEST(ConfigSettingTest, configSetStringValue) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  folly::StringPiece systemConfigDir{"/SYSTEM_CONFIG_SETTING"};
  std::map<std::string, std::string> attrMap;
  auto rslt = testDir.setStringValue(
      systemConfigDir, attrMap, ConfigSource::UserConfig);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::UserConfig);
  EXPECT_EQ(testDir.getValue(), systemConfigDir);
  EXPECT_EQ("/SYSTEM_CONFIG_SETTING", testDir.getStringValue());

  folly::StringPiece userConfigDir{"/USER_CONFIG_SETTING"};
  rslt =
      testDir.setStringValue(userConfigDir, attrMap, ConfigSource::UserConfig);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::UserConfig);
  EXPECT_EQ(testDir.getValue(), userConfigDir);
  EXPECT_EQ("/USER_CONFIG_SETTING", testDir.getStringValue());
}

TEST(ConfigSettingTest, configSetAssign) {
  // Setup our target copy
  AbsolutePath otherDir{"/OTHER_DIR"};
  auto otherKey = "otherKey"_sp;
  ConfigSetting<AbsolutePath> copyOfTestDir{otherKey, otherDir, nullptr};
  folly::StringPiece systemConfigDir{"/SYSTEM_CONFIG_SETTING"};

  // Check the copy states first, so we know where starting point is.
  EXPECT_EQ(copyOfTestDir.getConfigKey(), otherKey);
  EXPECT_EQ(copyOfTestDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(copyOfTestDir.getValue(), otherDir);

  auto dirKey = "dirKey"_sp;
  {
    // Setup the copy source - sufficiently different
    AbsolutePath defaultDir{"/DEFAULT_DIR"};
    ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

    std::map<std::string, std::string> attrMap;
    auto rslt = testDir.setStringValue(
        systemConfigDir, attrMap, ConfigSource::UserConfig);
    EXPECT_EQ(rslt.hasError(), false);

    EXPECT_EQ(testDir.getConfigKey(), dirKey);
    EXPECT_EQ(testDir.getSource(), ConfigSource::UserConfig);
    EXPECT_EQ(testDir.getValue(), systemConfigDir);

    copyOfTestDir.copyFrom(testDir);
  }

  // Check all attributes copied.
  EXPECT_EQ(copyOfTestDir.getConfigKey(), dirKey);
  EXPECT_EQ(copyOfTestDir.getSource(), ConfigSource::UserConfig);
  EXPECT_EQ(copyOfTestDir.getValue(), systemConfigDir);

  // Check references still valid
  copyOfTestDir.clearValue(ConfigSource::Default);
}

TEST(ConfigSettingTest, configSetInvalidStringValue) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  folly::StringPiece systemConfigDir{"/SYSTEM_CONFIG_SETTING"};
  std::map<std::string, std::string> attrMap;
  auto rslt = testDir.setStringValue(
      systemConfigDir, attrMap, ConfigSource::SystemConfig);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::SystemConfig);
  EXPECT_EQ(testDir.getValue(), systemConfigDir);

  folly::StringPiece userConfigDir{"INVALID USER_CONFIG_SETTING"};
  rslt =
      testDir.setStringValue(userConfigDir, attrMap, ConfigSource::UserConfig);
  EXPECT_EQ(rslt.hasError(), true);
  EXPECT_EQ(
      rslt.error(),
      "Cannot convert value 'INVALID USER_CONFIG_SETTING' to an absolute path");
  EXPECT_EQ(testDir.getSource(), ConfigSource::SystemConfig);
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
  auto rslt =
      testDir.setStringValue(userConfigDir, attrMap, ConfigSource::UserConfig);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::UserConfig);
  EXPECT_EQ(testDir.getValue(), "/home/bob/test_dir");
  EXPECT_EQ("/home/bob/test_dir", testDir.getStringValue());

  folly::StringPiece homeUserConfigDir{"/home/${USER}/test_dir"};
  rslt = testDir.setStringValue(
      homeUserConfigDir, attrMap, ConfigSource::UserConfig);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSource(), ConfigSource::UserConfig);
  EXPECT_EQ(testDir.getValue(), "/home/bob/test_dir");
  EXPECT_EQ("/home/bob/test_dir", testDir.getStringValue());
}

TEST(ConfigSettingTest, configSettingIgnoreDefault) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};
  // Initial should be default
  EXPECT_EQ(testDir.getValue(), defaultDir);
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);

  // Setting default value should be ignored
  AbsolutePath notDefaultDir{"/NOT_THE_DEFAULT_DIR"};
  testDir.setValue(notDefaultDir, ConfigSource::Default);
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir);

  // Clearing the default value should be ignored
  testDir.clearValue(ConfigSource::Default);
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir);
}

TEST(ConfigSettingTest, configSettingClearNonExistingSource) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);

  // Clear unset priorities
  testDir.clearValue(ConfigSource::CommandLine);
  testDir.clearValue(ConfigSource::UserConfig);
  testDir.clearValue(ConfigSource::SystemConfig);
  testDir.clearValue(ConfigSource::Default);

  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir);
}

TEST(ConfigSettingTest, configSettingSetAndClearTest) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  AbsolutePath systemEdenDir{"/SYSTEM_DIR"};

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir);

  // Over-ride default
  testDir.setValue(systemEdenDir, ConfigSource::SystemConfig);
  EXPECT_EQ(testDir.getSource(), ConfigSource::SystemConfig);
  EXPECT_EQ(testDir.getValue(), systemEdenDir);

  // Clear the over-ride
  testDir.clearValue(ConfigSource::SystemConfig);
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir);
}

TEST(ConfigSettingTest, configSetOverRiddenSource) {
  AbsolutePath defaultDir{"/DEFAULT_DIR"};
  auto dirKey = "dirKey"_sp;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  AbsolutePath cliEdenDir{"/CLI_DIR"};
  AbsolutePath systemEdenDir{"/SYSTEM_DIR"};

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);

  // Set the highest priority item
  testDir.setValue(cliEdenDir, ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getSource(), ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Set a middle priority item (results same as above)
  testDir.setValue(systemEdenDir, ConfigSource::SystemConfig);
  EXPECT_EQ(testDir.getSource(), ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear current highest priority
  testDir.clearValue(ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getSource(), ConfigSource::SystemConfig);
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
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir);

  // Set next higher over-ride priority
  testDir.setValue(systemEdenDir, ConfigSource::SystemConfig);
  EXPECT_EQ(testDir.getSource(), ConfigSource::SystemConfig);
  EXPECT_EQ(testDir.getValue(), systemEdenDir);

  // Set next higher over-ride priority
  testDir.setValue(userEdenDir, ConfigSource::UserConfig);
  EXPECT_EQ(testDir.getSource(), ConfigSource::UserConfig);
  EXPECT_EQ(testDir.getValue(), userEdenDir);

  // Set next higher over-ride priority
  testDir.setValue(cliEdenDir, ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getSource(), ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear the middle priority item (no effect on source/value)
  testDir.clearValue(ConfigSource::UserConfig);
  EXPECT_EQ(testDir.getSource(), ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear the middle priority item (no effect on source/value)
  testDir.clearValue(ConfigSource::SystemConfig);
  EXPECT_EQ(testDir.getSource(), ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear highest priority - back to default
  testDir.clearValue(ConfigSource::CommandLine);
  EXPECT_EQ(testDir.getSource(), ConfigSource::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir);
}

namespace {

template <typename T, typename FieldConverter, typename ExpectedType = T>
void checkSet(
    ConfigSetting<T, FieldConverter>& setting,
    ExpectedType expected,
    StringPiece str) {
  SCOPED_TRACE(str.str());
  std::map<std::string, std::string> attrMap;
  auto setResult =
      setting.setStringValue(str, attrMap, ConfigSource::UserConfig);
  ASSERT_FALSE(setResult.hasError()) << setResult.error();
  if constexpr (std::is_floating_point<T>::value) {
    EXPECT_FLOAT_EQ(expected, setting.getValue());
  } else {
    EXPECT_EQ(expected, setting.getValue());
  }
}

template <typename T, typename FieldConverter>
void checkSetError(
    ConfigSetting<T, FieldConverter>& setting,
    StringPiece expectedError,
    StringPiece str) {
  SCOPED_TRACE(str.str());
  std::map<std::string, std::string> attrMap;
  auto setResult =
      setting.setStringValue(str, attrMap, ConfigSource::UserConfig);
  ASSERT_TRUE(setResult.hasError());
  EXPECT_EQ(expectedError.str(), setResult.error());
}

} // namespace

TEST(ConfigSettingTest, setBool) {
  ConfigSetting<bool> defaultTrue{"test:value2", true, nullptr};
  EXPECT_EQ(true, defaultTrue.getValue());

  ConfigSetting<bool> setting{"test:value", false, nullptr};
  EXPECT_EQ(false, setting.getValue());

  checkSet(setting, true, "true");
  checkSet(setting, true, "1");
  checkSet(setting, true, "y");
  checkSet(setting, true, "yes");
  checkSet(setting, true, "Y");
  checkSet(setting, true, "on");
  EXPECT_EQ("true", setting.getStringValue());
  checkSet(setting, false, "n");
  checkSet(setting, false, "0");
  checkSet(setting, false, "false");
  checkSet(setting, false, "off");
  EXPECT_EQ("false", setting.getStringValue());

  checkSetError(setting, "Empty input string", "");
  checkSetError(setting, "Invalid value for bool: \"bogus\"", "bogus");
  checkSetError(
      setting,
      "Non-whitespace character found after end of conversion: \"yes_and\"",
      "yes_and");
}

TEST(ConfigSettingTest, setArithmetic) {
  ConfigSetting<int> intSetting{"test:value", 1, nullptr};
  EXPECT_EQ(1, intSetting.getValue());
  checkSet(intSetting, 9, "9");
  checkSet(intSetting, 1234, "1234");
  checkSetError(intSetting, "Empty input string", "");
  checkSetError(intSetting, "Invalid leading character: \"bogus\"", "bogus");
  // In the future it might be nice to support parsing hexadecimal input.
  checkSetError(
      intSetting,
      "Non-whitespace character found after end of conversion: \"0x15\"",
      "0x15");

  ConfigSetting<uint8_t> u8Setting{"test:value", 0, nullptr};
  checkSet(u8Setting, 9, "9");
  checkSetError(u8Setting, "Overflow during conversion: \"300\"", "300");
  checkSetError(u8Setting, "Non-digit character found: \"-10\"", "-10");

  ConfigSetting<float> floatSetting{"test:value", 0, nullptr};
  checkSet(floatSetting, 123.0, "123");
  checkSet(floatSetting, 0.001, "0.001");
  checkSetError(
      floatSetting,
      "Non-whitespace character found after end of conversion: \"0.001.9\"",
      "0.001.9");
}

TEST(ConfigSettingTest, setDuration) {
  ConfigSetting<std::chrono::nanoseconds> setting{"test:value", 5ms, nullptr};
  EXPECT_EQ(5ms, setting.getValue());
  checkSet(setting, 90s, "1m30s");
  checkSet(setting, -90s, "-1m30s");
  checkSet(setting, 42ns, "42ns");
  checkSet(setting, 300s, "5m");
  checkSetError(setting, "empty input string", "");
  checkSetError(setting, "unknown duration unit specifier", "90");
  checkSetError(setting, "non-digit character found", "bogus");
}
