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

#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/utils/PathFuncs.h"

using facebook::eden::AbsolutePath;
using facebook::eden::Hash;
using facebook::eden::RelativePath;
using folly::Optional;
using folly::StringPiece;

namespace {

using facebook::eden::EdenConfig;
using folly::test::TemporaryDirectory;

class EdenConfigTest : public ::testing::Test {
 protected:
  // Top level directory to hold test artifacts
  std::unique_ptr<TemporaryDirectory> rootTestDir_;

  // Default paths for when the path does not have to exist
  AbsolutePath testHomeDir_{"/home/bob"};
  AbsolutePath defaultUserConfigPath_{"/home/bob/.edenrc"};
  AbsolutePath defaultSystemConfigPath_{"/etc/eden/edenfs.rc"};

  // Used by various tests to verify default values is set
  AbsolutePath defaultUserIgnoreFilePath_{"/home/bob/ignore"};
  AbsolutePath defaultSystemIgnoreFilePath_{"/etc/eden/ignore"};
  AbsolutePath defaultEdenDirPath_{"/home/bob/.eden"};

  // Map of test names to system, user path
  std::map<std::string, std::pair<AbsolutePath, AbsolutePath>> testPathMap_;
  std::string simpleOverRideTest_{"simpleOverRideTest"};

  void SetUp() override {
    rootTestDir_ =
        std::make_unique<TemporaryDirectory>("eden_sys_user_config_test_");
    setupSimpleOverRideTest();
  }

  void TearDown() override {
    rootTestDir_.reset();
  }

  void setupSimpleOverRideTest() {
    auto testCaseDir = rootTestDir_->path() / simpleOverRideTest_;
    folly::fs::create_directory(testCaseDir);

    auto userConfigDir = testCaseDir / "client";
    folly::fs::create_directory(userConfigDir);

    auto userConfigPath = userConfigDir / ".edenrc";
    auto userConfigFileData = folly::StringPiece{
        "[core]\n"
        "ignoreFile=\"/home/bob/userCustomIgnore\"\n"};
    folly::writeFile(userConfigFileData, userConfigPath.c_str());

    auto systemConfigDir = testCaseDir / "etc-eden";
    folly::fs::create_directory(systemConfigDir);

    auto systemConfigPath = systemConfigDir / "edenfs.rc";
    auto systemConfigFileData = folly::StringPiece{
        "[core]\n"
        "ignoreFile=\"/should_be_over_ridden\"\n"
        "systemIgnoreFile=\"/etc/eden/systemCustomIgnore\"\n"};
    folly::writeFile(systemConfigFileData, systemConfigPath.c_str());

    testPathMap_[simpleOverRideTest_] = std::pair<AbsolutePath, AbsolutePath>(
        AbsolutePath{systemConfigPath.string()},
        AbsolutePath{userConfigPath.string()});
  }
};
} // namespace

TEST_F(EdenConfigTest, defaultTest) {
  AbsolutePath userConfigPath{"/home/bob/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden"};

  auto edenConfig = std::make_shared<facebook::eden::EdenConfig>(
      testHomeDir_, userConfigPath, systemConfigDir, systemConfigPath);

  // Config path
  EXPECT_EQ(edenConfig->getUserConfigPath(), userConfigPath);
  EXPECT_EQ(edenConfig->getSystemConfigPath(), systemConfigPath);
  EXPECT_EQ(edenConfig->getSystemConfigDir(), systemConfigDir);

  // Configuration
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
}

TEST_F(EdenConfigTest, simpleSetGetTest) {
  AbsolutePath userConfigPath{"/home/bob/differentConfigPath/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/fix/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden/fix"};

  auto edenConfig = std::make_shared<facebook::eden::EdenConfig>(
      testHomeDir_, userConfigPath, systemConfigPath, systemConfigDir);

  AbsolutePath ignoreFile{"/home/bob/alternativeIgnore"};
  AbsolutePath systemIgnoreFile{"/etc/eden/fix/systemIgnore"};
  AbsolutePath edenDir{"/home/bob/alt/.eden"};

  AbsolutePath updatedUserConfigPath{
      "/home/bob/differentConfigPath/.edenrcUPDATED"};
  AbsolutePath updatedSystemConfigPath{"/etc/eden/fix/edenfs.rcUPDATED"};
  AbsolutePath updatedSystemConfigDir{"/etc/eden/fixUPDATED"};

  // Config path
  edenConfig->setUserConfigPath(updatedUserConfigPath);
  edenConfig->setSystemConfigDir(updatedSystemConfigDir);
  edenConfig->setSystemConfigPath(updatedSystemConfigPath);

  // Configuration
  edenConfig->setUserIgnoreFile(ignoreFile, facebook::eden::COMMAND_LINE);
  edenConfig->setSystemIgnoreFile(
      systemIgnoreFile, facebook::eden::COMMAND_LINE);
  edenConfig->setEdenDir(edenDir, facebook::eden::COMMAND_LINE);

  // Config path
  EXPECT_EQ(edenConfig->getUserConfigPath(), updatedUserConfigPath);
  EXPECT_EQ(edenConfig->getSystemConfigDir(), updatedSystemConfigDir);
  EXPECT_EQ(edenConfig->getSystemConfigPath(), updatedSystemConfigPath);

  // Configuration
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), ignoreFile);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), systemIgnoreFile);
  EXPECT_EQ(edenConfig->getEdenDir(), edenDir);
}

TEST_F(EdenConfigTest, cloneTest) {
  AbsolutePath userConfigPath{"/home/bob/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden"};

  AbsolutePath ignoreFile{"/NON_DEFAULT_IGNORE_FILE"};
  AbsolutePath systemIgnoreFile{"/NON_DEFAULT_SYSTEM_IGNORE_FILE"};
  AbsolutePath edenDir{"/NON_DEFAULT_EDEN_DIR"};

  std::shared_ptr<EdenConfig> configCopy;
  {
    auto edenConfig = std::make_shared<facebook::eden::EdenConfig>(
        testHomeDir_, userConfigPath, systemConfigDir, systemConfigPath);

    // Configuration
    edenConfig->setUserIgnoreFile(ignoreFile, facebook::eden::COMMAND_LINE);
    edenConfig->setSystemIgnoreFile(
        systemIgnoreFile, facebook::eden::SYSTEM_CONFIG_FILE);
    edenConfig->setEdenDir(edenDir, facebook::eden::USER_CONFIG_FILE);

    EXPECT_EQ(edenConfig->getUserConfigPath(), userConfigPath);
    EXPECT_EQ(edenConfig->getSystemConfigPath(), systemConfigPath);
    EXPECT_EQ(edenConfig->getSystemConfigDir(), systemConfigDir);

    EXPECT_EQ(edenConfig->getUserIgnoreFile(), ignoreFile);
    EXPECT_EQ(edenConfig->getSystemIgnoreFile(), systemIgnoreFile);
    EXPECT_EQ(edenConfig->getEdenDir(), edenDir);

    configCopy = std::make_shared<EdenConfig>(*edenConfig);
  }

  EXPECT_EQ(configCopy->getUserConfigPath(), userConfigPath);
  EXPECT_EQ(configCopy->getSystemConfigPath(), systemConfigPath);
  EXPECT_EQ(configCopy->getSystemConfigDir(), systemConfigDir);

  EXPECT_EQ(configCopy->getUserIgnoreFile(), ignoreFile);
  EXPECT_EQ(configCopy->getSystemIgnoreFile(), systemIgnoreFile);
  EXPECT_EQ(configCopy->getEdenDir(), edenDir);

  configCopy->clearAll(facebook::eden::USER_CONFIG_FILE);
  configCopy->clearAll(facebook::eden::SYSTEM_CONFIG_FILE);
  configCopy->clearAll(facebook::eden::COMMAND_LINE);

  EXPECT_EQ(configCopy->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(configCopy->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(configCopy->getEdenDir(), defaultEdenDirPath_);
}

TEST_F(EdenConfigTest, clearAllTest) {
  AbsolutePath userConfigPath{"/home/bob/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden"};

  auto edenConfig = std::make_shared<facebook::eden::EdenConfig>(
      testHomeDir_, userConfigPath, systemConfigDir, systemConfigPath);

  AbsolutePath fromUserConfigPath{"/home/bob/FROM_USER_CONFIG"};
  AbsolutePath fromSystemConfigPath{"/etc/eden/FROM_SYSTEM_CONFIG"};
  AbsolutePath fromCommandLine{"/home/bob/alt/FROM_COMMAND_LINE"};

  // We will set the config on 3 properties, each with different sources
  // We will then run for each source to check results
  edenConfig->setUserIgnoreFile(
      fromUserConfigPath, facebook::eden::USER_CONFIG_FILE);
  edenConfig->setSystemIgnoreFile(
      fromSystemConfigPath, facebook::eden::SYSTEM_CONFIG_FILE);
  edenConfig->setEdenDir(fromCommandLine, facebook::eden::COMMAND_LINE);
  edenConfig->setEdenDir(fromUserConfigPath, facebook::eden::USER_CONFIG_FILE);
  edenConfig->setEdenDir(
      fromSystemConfigPath, facebook::eden::SYSTEM_CONFIG_FILE);

  // Check over-rides
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), fromUserConfigPath);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), fromSystemConfigPath);
  EXPECT_EQ(edenConfig->getEdenDir(), fromCommandLine);

  // Clear USER_CONFIG_FILE and check
  edenConfig->clearAll(facebook::eden::USER_CONFIG_FILE);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), fromSystemConfigPath);
  EXPECT_EQ(edenConfig->getEdenDir(), fromCommandLine);

  // Clear SYSTEM_CONFIG_FILE and check
  edenConfig->clearAll(facebook::eden::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getEdenDir(), fromCommandLine);

  // Clear COMMAND_LINE and check
  edenConfig->clearAll(facebook::eden::COMMAND_LINE);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
}

TEST_F(EdenConfigTest, overRideNotAllowedTest) {
  AbsolutePath userConfigPath{"/home/bob/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden"};

  auto edenConfig = std::make_shared<facebook::eden::EdenConfig>(
      testHomeDir_, userConfigPath, systemConfigDir, systemConfigPath);

  // Check default (starting point)
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), "/home/bob/ignore");

  // Set from cli and verify that cannot over-ride
  AbsolutePath cliIgnoreFile{"/CLI_IGNORE_FILE"};
  AbsolutePath ignoreFile{"/USER_IGNORE_FILE"};
  AbsolutePath systemIgnoreFile{"/SYSTEM_IGNORE_FILE"};

  edenConfig->setUserIgnoreFile(cliIgnoreFile, facebook::eden::COMMAND_LINE);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), cliIgnoreFile);

  edenConfig->setUserIgnoreFile(
      cliIgnoreFile, facebook::eden::SYSTEM_CONFIG_FILE);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), cliIgnoreFile);

  edenConfig->setUserIgnoreFile(ignoreFile, facebook::eden::USER_CONFIG_FILE);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), cliIgnoreFile);
}

TEST_F(EdenConfigTest, loadSystemUserConfigTest) {
  // TODO: GET THE BASE NAME FOR THE SYSTEM CONFIG DIR!
  auto edenConfig = std::make_shared<facebook::eden::EdenConfig>(
      testHomeDir_,
      testPathMap_[simpleOverRideTest_].second,
      testPathMap_[simpleOverRideTest_].first,
      testPathMap_[simpleOverRideTest_].first);

  edenConfig->loadSystemConfig();

  EXPECT_EQ(edenConfig->getUserIgnoreFile(), "/should_be_over_ridden");
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), "/etc/eden/systemCustomIgnore");
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);

  edenConfig->loadUserConfig();

  EXPECT_EQ(edenConfig->getUserIgnoreFile(), "/home/bob/userCustomIgnore");
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), "/etc/eden/systemCustomIgnore");
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
}

TEST_F(EdenConfigTest, nonExistingConfigFiles) {
  auto userConfigPath = AbsolutePath{"/home/bob/.FILE_DOES_NOT_EXIST"};
  auto systemConfigDir = AbsolutePath{"/etc/eden"};
  auto systemConfigPath = AbsolutePath{"/etc/eden/FILE_DOES_NOT_EXIST.rc"};

  auto edenConfig = std::make_shared<facebook::eden::EdenConfig>(
      testHomeDir_, userConfigPath, systemConfigDir, systemConfigPath);

  edenConfig->loadSystemConfig();
  edenConfig->loadUserConfig();

  // Check default configuration is set
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
}
