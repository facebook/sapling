/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/utils/PathFuncs.h"

#include <folly/experimental/TestUtil.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using namespace std::string_view_literals;

class ConfigSettingTest : public ::testing::Test {
 protected:
  static constexpr std::string_view defaultDirSp_{"/DEFAULT_DIR"};
  static constexpr std::string_view systemConfigDirSp_{
      "/SYSTEM_CONFIG_SETTING"};
  static constexpr std::string_view userConfigDirSp_{"/USER_CONFIG_SETTING"};
  static constexpr std::string_view otherDirSp_{"/OTHER_DIR"};
  AbsolutePath defaultDir_;
  AbsolutePath systemConfigDir_;
  AbsolutePath userConfigDir_;
  AbsolutePath otherDir_;

  void SetUp() override {
    defaultDir_ = normalizeBestEffort(defaultDirSp_);
    systemConfigDir_ = normalizeBestEffort(systemConfigDirSp_);
    userConfigDir_ = normalizeBestEffort(userConfigDirSp_);
    otherDir_ = normalizeBestEffort(otherDirSp_);
  }
};

TEST_F(ConfigSettingTest, initStateCheck) {
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};

  // Initial should be default
  EXPECT_EQ(testDir.getValue(), defaultDir_);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(testDir.getConfigKey(), dirKey);
}

TEST_F(ConfigSettingTest, configSetStringValue) {
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};

  std::map<std::string, std::string> attrMap;
  auto rslt = testDir.setStringValue(
      systemConfigDir_.view(), attrMap, ConfigSourceType::UserConfig);
  rslt.value();
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::UserConfig);
  EXPECT_EQ(testDir.getValue(), systemConfigDir_);
  EXPECT_EQ(systemConfigDir_, testDir.getStringValue());

  rslt = testDir.setStringValue(
      userConfigDir_.view(), attrMap, ConfigSourceType::UserConfig);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::UserConfig);
  EXPECT_EQ(testDir.getValue(), userConfigDir_);
  EXPECT_EQ(userConfigDir_, testDir.getStringValue());
}

TEST_F(ConfigSettingTest, configSetAssign) {
  // Setup our target copy
  auto otherKey = "otherKey"sv;
  ConfigSetting<AbsolutePath> copyOfTestDir{otherKey, otherDir_, nullptr};

  // Check the copy states first, so we know where starting point is.
  EXPECT_EQ(copyOfTestDir.getConfigKey(), otherKey);
  EXPECT_EQ(copyOfTestDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(copyOfTestDir.getValue(), otherDir_);

  auto dirKey = "dirKey"sv;
  {
    // Setup the copy source - sufficiently different
    ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};

    std::map<std::string, std::string> attrMap;
    auto rslt = testDir.setStringValue(
        systemConfigDir_.view(), attrMap, ConfigSourceType::UserConfig);
    EXPECT_EQ(rslt.hasError(), false);

    EXPECT_EQ(testDir.getConfigKey(), dirKey);
    EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::UserConfig);
    EXPECT_EQ(testDir.getValue(), systemConfigDir_);

    copyOfTestDir.copyFrom(testDir);
  }

  // Check all attributes copied.
  EXPECT_EQ(copyOfTestDir.getConfigKey(), dirKey);
  EXPECT_EQ(copyOfTestDir.getSourceType(), ConfigSourceType::UserConfig);
  EXPECT_EQ(copyOfTestDir.getValue(), systemConfigDir_);

  // Check references still valid
  copyOfTestDir.clearValue(ConfigSourceType::Default);
}

TEST_F(ConfigSettingTest, configSetInvalidStringValue) {
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};

  std::map<std::string, std::string> attrMap;
  auto rslt = testDir.setStringValue(
      systemConfigDir_.view(), attrMap, ConfigSourceType::SystemConfig);
  EXPECT_EQ(rslt.hasError(), false);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getValue(), systemConfigDir_);

  std::string_view userConfigDir{"INVALID USER_CONFIG_SETTING/"};
  rslt = testDir.setStringValue(
      userConfigDir, attrMap, ConfigSourceType::UserConfig);
  EXPECT_EQ(rslt.hasError(), true);
  EXPECT_EQ(
      rslt.error(),
      "Cannot convert value 'INVALID USER_CONFIG_SETTING/' to an absolute path");
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getValue(), systemConfigDir_);
}

TEST_F(ConfigSettingTest, configSetEnvSubTest) {
  AbsolutePath defaultDir = canonicalPath("/home/bob");
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir, nullptr};

  std::string_view userConfigDir{"${HOME}/test_dir"};
  std::map<std::string, std::string> attrMap;
  attrMap["HOME"] = canonicalPath("/home/bob").asString();
  attrMap["USER"] = "bob";
  auto rslt = testDir.setStringValue(
      userConfigDir, attrMap, ConfigSourceType::UserConfig);
  EXPECT_EQ(rslt.hasError(), false) << rslt.error();
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::UserConfig);
  AbsolutePath bobTestDir = canonicalPath("/home/bob/test_dir");
  EXPECT_EQ(testDir.getValue(), bobTestDir);
  EXPECT_EQ(bobTestDir, testDir.getStringValue());

  AbsolutePath homeUserConfigDir = canonicalPath("/home/${USER}/test_dir");
  rslt = testDir.setStringValue(
      homeUserConfigDir.view(), attrMap, ConfigSourceType::UserConfig);
  EXPECT_EQ(rslt.hasError(), false) << rslt.error();
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::UserConfig);
  EXPECT_EQ(testDir.getValue(), bobTestDir);
  EXPECT_EQ(bobTestDir, testDir.getStringValue());
}

TEST_F(ConfigSettingTest, configSettingIgnoreDefault) {
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};
  // Initial should be default
  EXPECT_EQ(testDir.getValue(), defaultDir_);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);

  // Setting default value should be ignored
  AbsolutePath notDefaultDir = canonicalPath("/NOT_THE_DEFAULT_DIR");
  testDir.setValue(notDefaultDir, ConfigSourceType::Default);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir_);

  // Clearing the default value should be ignored
  testDir.clearValue(ConfigSourceType::Default);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir_);
}

TEST_F(ConfigSettingTest, configSettingClearNonExistingSource) {
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);

  // Clear unset priorities
  testDir.clearValue(ConfigSourceType::CommandLine);
  testDir.clearValue(ConfigSourceType::UserConfig);
  testDir.clearValue(ConfigSourceType::SystemConfig);
  testDir.clearValue(ConfigSourceType::Default);

  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir_);
}

TEST_F(ConfigSettingTest, configSettingSetAndClearTest) {
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};

  AbsolutePath systemEdenDir = canonicalPath("/SYSTEM_DIR");

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir_);

  // Over-ride default
  testDir.setValue(systemEdenDir, ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getValue(), systemEdenDir);

  // Clear the over-ride
  testDir.clearValue(ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir_);
}

TEST_F(ConfigSettingTest, configSetOverRiddenSource) {
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};

  AbsolutePath cliEdenDir = canonicalPath("/CLI_DIR");
  AbsolutePath systemEdenDir = canonicalPath("/SYSTEM_DIR");

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);

  // Set the highest priority item
  testDir.setValue(cliEdenDir, ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Set a middle priority item (results same as above)
  testDir.setValue(systemEdenDir, ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear current highest priority
  testDir.clearValue(ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getValue(), systemEdenDir);
}

TEST_F(ConfigSettingTest, configClearOverRiddenSource) {
  auto dirKey = "dirKey"sv;
  ConfigSetting<AbsolutePath> testDir{dirKey, defaultDir_, nullptr};

  AbsolutePath cliEdenDir = canonicalPath("/CLI_DIR");
  AbsolutePath userEdenDir = canonicalPath("/USER_DIR");
  AbsolutePath systemEdenDir = canonicalPath("/SYSTEM_DIR");

  // Initially, it should be default value
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir_);

  // Set next higher over-ride priority
  testDir.setValue(systemEdenDir, ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getValue(), systemEdenDir);

  // Set next higher over-ride priority
  testDir.setValue(userEdenDir, ConfigSourceType::UserConfig);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::UserConfig);
  EXPECT_EQ(testDir.getValue(), userEdenDir);

  // Set next higher over-ride priority
  testDir.setValue(cliEdenDir, ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear the middle priority item (no effect on source/value)
  testDir.clearValue(ConfigSourceType::UserConfig);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear the middle priority item (no effect on source/value)
  testDir.clearValue(ConfigSourceType::SystemConfig);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getValue(), cliEdenDir);

  // Clear highest priority - back to default
  testDir.clearValue(ConfigSourceType::CommandLine);
  EXPECT_EQ(testDir.getSourceType(), ConfigSourceType::Default);
  EXPECT_EQ(testDir.getValue(), defaultDir_);
}

namespace {

template <typename T, typename FieldConverter, typename ExpectedType = T>
void checkSet(
    ConfigSetting<T, FieldConverter>& setting,
    ExpectedType expected,
    std::string_view str) {
  SCOPED_TRACE(str);
  std::map<std::string, std::string> attrMap;
  auto setResult =
      setting.setStringValue(str, attrMap, ConfigSourceType::UserConfig);
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
    std::string_view expectedError,
    std::string_view str) {
  SCOPED_TRACE(str);
  std::map<std::string, std::string> attrMap;
  auto setResult =
      setting.setStringValue(str, attrMap, ConfigSourceType::UserConfig);
  ASSERT_TRUE(setResult.hasError());
  EXPECT_EQ(expectedError, setResult.error());
}

} // namespace

TEST_F(ConfigSettingTest, setBool) {
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

TEST_F(ConfigSettingTest, setArithmetic) {
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
  checkSet(floatSetting, 123.0f, "123");
  checkSet(floatSetting, 0.001f, "0.001");
  checkSetError(
      floatSetting,
      "Non-whitespace character found after end of conversion: \"0.001.9\"",
      "0.001.9");
}

TEST_F(ConfigSettingTest, setDuration) {
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

TEST_F(ConfigSettingTest, setArray) {
  ConfigSetting<std::vector<std::string>> setting{
      "test:value", std::vector<std::string>{}, nullptr};
  EXPECT_EQ(std::vector<std::string>{}, setting.getValue());
  checkSet(setting, std::vector<std::string>{"a"}, "[\"a\"]");
  checkSet(setting, std::vector<std::string>{"a", "b"}, "[\"a\", \"b\"]");
  checkSet(setting, std::vector<std::string>{}, "[]");
  checkSetError(
      setting,
      "Error parsing an array of strings: Failed to parse value type at line 1",
      "");
  checkSetError(
      setting,
      "Error parsing an array of strings: Unidentified trailing character ','--"
      "-did you forget a '#'? at line 1",
      "\"a\", \"b\", \"c\"");
}

TEST_F(ConfigSettingTest, setArrayDuration) {
  ConfigSetting<std::vector<std::chrono::nanoseconds>> setting{
      "test:value", std::vector<std::chrono::nanoseconds>{}, nullptr};
  checkSet(setting, std::vector<std::chrono::nanoseconds>{90s}, "[\"1m30s\"]");
}

TEST_F(ConfigSettingTest, setArrayOptional) {
  ConfigSetting<std::vector<std::optional<std::string>>> setting{
      "test:value", std::vector<std::optional<std::string>>{}, nullptr};
  checkSet(
      setting, std::vector<std::optional<std::string>>{"foo"}, "[\"foo\"]");
}
