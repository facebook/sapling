/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/EdenConfig.h"

#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/utils/PathFuncs.h"

using folly::StringPiece;
using folly::test::TemporaryDirectory;
using std::optional;
using namespace folly::literals::string_piece_literals;
using namespace facebook::eden;
using namespace facebook::eden::path_literals;

namespace {

class EdenConfigTest : public ::testing::Test {
 protected:
  // Top level directory to hold test artifacts
  std::unique_ptr<TemporaryDirectory> rootTestDir_;

  // Default paths for when the path does not have to exist
  std::string testUser_{"bob"};
  AbsolutePath testHomeDir_{"/home/bob"};
  AbsolutePath defaultUserConfigPath_{"/home/bob/.edenrc"};
  AbsolutePath defaultSystemConfigPath_{"/etc/eden/edenfs.rc"};

  // Used by various tests to verify default values is set
  AbsolutePath defaultUserIgnoreFilePath_{"/home/bob/.edenignore"};
  AbsolutePath defaultSystemIgnoreFilePath_{"/etc/eden/ignore"};
  AbsolutePath defaultEdenDirPath_{"/home/bob/.eden"};
  optional<AbsolutePath> defaultClientCertificatePath_;
  bool defaultUseMononoke_ = false;

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
        "ignoreFile=\"${HOME}/${USER}/userCustomIgnore\"\n"
        "[mononoke]\n"
        "use-mononoke=\"false\""};
    folly::writeFile(userConfigFileData, userConfigPath.c_str());

    auto systemConfigDir = testCaseDir / "etc-eden";
    folly::fs::create_directory(systemConfigDir);

    auto systemConfigPath = systemConfigDir / "edenfs.rc";
    auto systemConfigFileData = folly::StringPiece{
        "[core]\n"
        "ignoreFile=\"/should_be_over_ridden\"\n"
        "systemIgnoreFile=\"/etc/eden/systemCustomIgnore\"\n"
        "[mononoke]\n"
        "use-mononoke=true\n"
        "[ssl]\n"
        "client-certificate=\"/system_config_cert/${USER}/foo/${USER}\"\n"};
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

  auto edenConfig = std::make_shared<EdenConfig>(
      testUser_,
      uid_t{},
      testHomeDir_,
      userConfigPath,
      systemConfigDir,
      systemConfigPath);

  // Config path
  EXPECT_EQ(edenConfig->getUserConfigPath(), userConfigPath);
  EXPECT_EQ(edenConfig->getSystemConfigPath(), systemConfigPath);
  EXPECT_EQ(edenConfig->getSystemConfigDir(), systemConfigDir);

  // Configuration
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
  EXPECT_EQ(edenConfig->getClientCertificate(), defaultClientCertificatePath_);
  EXPECT_EQ(edenConfig->getUseMononoke(), defaultUseMononoke_);
}

TEST_F(EdenConfigTest, simpleSetGetTest) {
  AbsolutePath userConfigPath{"/home/bob/differentConfigPath/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/fix/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden/fix"};

  auto edenConfig = std::make_shared<EdenConfig>(
      testUser_,
      uid_t{},
      testHomeDir_,
      userConfigPath,
      systemConfigPath,
      systemConfigDir);

  AbsolutePath ignoreFile{"/home/bob/alternativeIgnore"};
  AbsolutePath systemIgnoreFile{"/etc/eden/fix/systemIgnore"};
  AbsolutePath edenDir{"/home/bob/alt/.eden"};
  AbsolutePath clientCertificate{"/home/bob/client.pem"};
  bool useMononoke = true;

  AbsolutePath updatedUserConfigPath{
      "/home/bob/differentConfigPath/.edenrcUPDATED"};
  AbsolutePath updatedSystemConfigPath{"/etc/eden/fix/edenfs.rcUPDATED"};
  AbsolutePath updatedSystemConfigDir{"/etc/eden/fixUPDATED"};

  // Config path
  edenConfig->setUserConfigPath(updatedUserConfigPath);
  edenConfig->setSystemConfigDir(updatedSystemConfigDir);
  edenConfig->setSystemConfigPath(updatedSystemConfigPath);

  // Configuration
  edenConfig->setUserIgnoreFile(ignoreFile, ConfigSource::CommandLine);
  edenConfig->setSystemIgnoreFile(systemIgnoreFile, ConfigSource::CommandLine);
  edenConfig->setEdenDir(edenDir, ConfigSource::CommandLine);
  edenConfig->setClientCertificate(
      clientCertificate, ConfigSource::CommandLine);
  edenConfig->setUseMononoke(useMononoke, ConfigSource::CommandLine);

  // Config path
  EXPECT_EQ(edenConfig->getUserConfigPath(), updatedUserConfigPath);
  EXPECT_EQ(edenConfig->getSystemConfigDir(), updatedSystemConfigDir);
  EXPECT_EQ(edenConfig->getSystemConfigPath(), updatedSystemConfigPath);

  // Configuration
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), ignoreFile);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), systemIgnoreFile);
  EXPECT_EQ(edenConfig->getEdenDir(), edenDir);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate);
  EXPECT_EQ(edenConfig->getUseMononoke(), useMononoke);
}

TEST_F(EdenConfigTest, cloneTest) {
  uid_t userID{};
  AbsolutePath userConfigPath{"/home/bob/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden"};

  AbsolutePath ignoreFile{"/NON_DEFAULT_IGNORE_FILE"};
  AbsolutePath systemIgnoreFile{"/NON_DEFAULT_SYSTEM_IGNORE_FILE"};
  AbsolutePath edenDir{"/NON_DEFAULT_EDEN_DIR"};
  AbsolutePath clientCertificate{"/NON_DEFAULT_CLIENT_CERTIFICATE"};
  bool useMononoke = true;

  std::shared_ptr<EdenConfig> configCopy;
  {
    auto edenConfig = std::make_shared<EdenConfig>(
        testUser_,
        userID,
        testHomeDir_,
        userConfigPath,
        systemConfigDir,
        systemConfigPath);

    // Configuration
    edenConfig->setUserIgnoreFile(ignoreFile, ConfigSource::CommandLine);
    edenConfig->setSystemIgnoreFile(
        systemIgnoreFile, ConfigSource::SystemConfig);
    edenConfig->setEdenDir(edenDir, ConfigSource::UserConfig);
    edenConfig->setClientCertificate(
        clientCertificate, ConfigSource::UserConfig);
    edenConfig->setUseMononoke(useMononoke, ConfigSource::UserConfig);

    EXPECT_EQ(edenConfig->getUserName(), testUser_);
    EXPECT_EQ(edenConfig->getUserID(), userID);
    EXPECT_EQ(edenConfig->getUserConfigPath(), userConfigPath);
    EXPECT_EQ(edenConfig->getSystemConfigPath(), systemConfigPath);
    EXPECT_EQ(edenConfig->getSystemConfigDir(), systemConfigDir);

    EXPECT_EQ(edenConfig->getUserIgnoreFile(), ignoreFile);
    EXPECT_EQ(edenConfig->getSystemIgnoreFile(), systemIgnoreFile);
    EXPECT_EQ(edenConfig->getEdenDir(), edenDir);
    EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate);
    EXPECT_EQ(edenConfig->getUseMononoke(), useMononoke);

    configCopy = std::make_shared<EdenConfig>(*edenConfig);
  }

  EXPECT_EQ(configCopy->getUserName(), testUser_);
  EXPECT_EQ(configCopy->getUserID(), userID);
  EXPECT_EQ(configCopy->getUserConfigPath(), userConfigPath);
  EXPECT_EQ(configCopy->getSystemConfigPath(), systemConfigPath);
  EXPECT_EQ(configCopy->getSystemConfigDir(), systemConfigDir);

  EXPECT_EQ(configCopy->getUserIgnoreFile(), ignoreFile);
  EXPECT_EQ(configCopy->getSystemIgnoreFile(), systemIgnoreFile);
  EXPECT_EQ(configCopy->getEdenDir(), edenDir);
  EXPECT_EQ(configCopy->getClientCertificate(), clientCertificate);
  EXPECT_EQ(configCopy->getUseMononoke(), useMononoke);

  configCopy->clearAll(ConfigSource::UserConfig);
  configCopy->clearAll(ConfigSource::SystemConfig);
  configCopy->clearAll(ConfigSource::CommandLine);

  EXPECT_EQ(configCopy->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(configCopy->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(configCopy->getEdenDir(), defaultEdenDirPath_);
  EXPECT_EQ(configCopy->getClientCertificate(), defaultClientCertificatePath_);
  EXPECT_EQ(configCopy->getUseMononoke(), defaultUseMononoke_);
}

TEST_F(EdenConfigTest, clearAllTest) {
  AbsolutePath userConfigPath{"/home/bob/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden"};

  auto edenConfig = std::make_shared<EdenConfig>(
      testUser_,
      uid_t{},
      testHomeDir_,
      userConfigPath,
      systemConfigDir,
      systemConfigPath);

  AbsolutePath fromUserConfigPath{"/home/bob/FROM_USER_CONFIG"};
  AbsolutePath fromSystemConfigPath{"/etc/eden/FROM_SYSTEM_CONFIG"};
  AbsolutePath fromCommandLine{"/home/bob/alt/FROM_COMMAND_LINE"};

  // We will set the config on 3 properties, each with different sources
  // We will then run for each source to check results
  edenConfig->setUserIgnoreFile(fromUserConfigPath, ConfigSource::UserConfig);
  edenConfig->setSystemIgnoreFile(
      fromSystemConfigPath, ConfigSource::SystemConfig);
  edenConfig->setEdenDir(fromCommandLine, ConfigSource::CommandLine);
  edenConfig->setEdenDir(fromUserConfigPath, ConfigSource::UserConfig);
  edenConfig->setEdenDir(fromSystemConfigPath, ConfigSource::SystemConfig);

  // Check over-rides
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), fromUserConfigPath);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), fromSystemConfigPath);
  EXPECT_EQ(edenConfig->getEdenDir(), fromCommandLine);

  // Clear UserConfig and check
  edenConfig->clearAll(ConfigSource::UserConfig);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), fromSystemConfigPath);
  EXPECT_EQ(edenConfig->getEdenDir(), fromCommandLine);

  // Clear SystemConfig and check
  edenConfig->clearAll(ConfigSource::SystemConfig);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getEdenDir(), fromCommandLine);

  // Clear CommandLine and check
  edenConfig->clearAll(ConfigSource::CommandLine);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
}

TEST_F(EdenConfigTest, overRideNotAllowedTest) {
  AbsolutePath userConfigPath{"/home/bob/.edenrc"};
  AbsolutePath systemConfigPath{"/etc/eden/edenfs.rc"};
  AbsolutePath systemConfigDir{"/etc/eden"};

  auto edenConfig = std::make_shared<EdenConfig>(
      testUser_,
      uid_t{},
      testHomeDir_,
      userConfigPath,
      systemConfigDir,
      systemConfigPath);

  // Check default (starting point)
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), "/home/bob/.edenignore");

  // Set from cli and verify that cannot over-ride
  AbsolutePath cliIgnoreFile{"/CLI_IGNORE_FILE"};
  AbsolutePath ignoreFile{"/USER_IGNORE_FILE"};
  AbsolutePath systemIgnoreFile{"/SYSTEM_IGNORE_FILE"};

  edenConfig->setUserIgnoreFile(cliIgnoreFile, ConfigSource::CommandLine);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), cliIgnoreFile);

  edenConfig->setUserIgnoreFile(cliIgnoreFile, ConfigSource::SystemConfig);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), cliIgnoreFile);

  edenConfig->setUserIgnoreFile(ignoreFile, ConfigSource::UserConfig);
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), cliIgnoreFile);
}

TEST_F(EdenConfigTest, loadSystemUserConfigTest) {
  // TODO: GET THE BASE NAME FOR THE SYSTEM CONFIG DIR!
  auto edenConfig = std::make_shared<EdenConfig>(
      testUser_,
      uid_t{},
      testHomeDir_,
      testPathMap_[simpleOverRideTest_].second,
      testPathMap_[simpleOverRideTest_].first,
      testPathMap_[simpleOverRideTest_].first);

  edenConfig->loadSystemConfig();

  EXPECT_EQ(edenConfig->getUserIgnoreFile(), "/should_be_over_ridden");
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), "/etc/eden/systemCustomIgnore");
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
  EXPECT_EQ(
      edenConfig->getClientCertificate()->stringPiece(),
      "/system_config_cert/bob/foo/bob");
  EXPECT_EQ(edenConfig->getUseMononoke(), true);

  edenConfig->loadUserConfig();

  EXPECT_EQ(edenConfig->getUserIgnoreFile(), "/home/bob/bob/userCustomIgnore");
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), "/etc/eden/systemCustomIgnore");
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
  EXPECT_EQ(
      edenConfig->getClientCertificate()->stringPiece(),
      "/system_config_cert/bob/foo/bob");
  EXPECT_EQ(edenConfig->getUseMononoke(), false);
}

TEST_F(EdenConfigTest, nonExistingConfigFiles) {
  auto userConfigPath = AbsolutePath{"/home/bob/.FILE_DOES_NOT_EXIST"};
  auto systemConfigDir = AbsolutePath{"/etc/eden"};
  auto systemConfigPath = AbsolutePath{"/etc/eden/FILE_DOES_NOT_EXIST.rc"};

  auto edenConfig = std::make_shared<EdenConfig>(
      testUser_,
      uid_t{},
      testHomeDir_,
      userConfigPath,
      systemConfigDir,
      systemConfigPath);

  edenConfig->loadSystemConfig();
  edenConfig->loadUserConfig();

  // Check default configuration is set
  EXPECT_EQ(edenConfig->getUserIgnoreFile(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getSystemIgnoreFile(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->getEdenDir(), defaultEdenDirPath_);
  EXPECT_EQ(edenConfig->getClientCertificate(), defaultClientCertificatePath_);
  EXPECT_EQ(edenConfig->getUseMononoke(), defaultUseMononoke_);
}

TEST_F(EdenConfigTest, variablesExpandInPathOptions) {
  auto systemConfigDir = rootTestDir_->path() / "etc-eden";
  folly::fs::create_directory(systemConfigDir);

  auto userConfigPath = rootTestDir_->path() / "user-edenrc";
  auto getConfig = [&]() {
    auto config =
        EdenConfig{"testusername"_sp,
                   uid_t{42},
                   AbsolutePath{"/testhomedir"_abspath},
                   AbsolutePath{userConfigPath.string()},
                   AbsolutePath{systemConfigDir.string()},
                   AbsolutePath{(systemConfigDir / "system-edenrc").string()}};
    config.loadUserConfig();
    return EdenConfig{config};
  };

  folly::writeFile(
      "[core]\n"
      "ignoreFile=\"${HOME}/myignore\"\n"_sp,
      userConfigPath.c_str());
  EXPECT_EQ(getConfig().getUserIgnoreFile(), "/testhomedir/myignore");

  folly::writeFile(
      "[core]\n"
      "ignoreFile=\"/home/${USER}/myignore\"\n"_sp,
      userConfigPath.c_str());
  EXPECT_EQ(getConfig().getUserIgnoreFile(), "/home/testusername/myignore");

  folly::writeFile(
      "[core]\n"
      "ignoreFile=\"/var/user/${USER_ID}/myignore\"\n"_sp,
      userConfigPath.c_str());
  EXPECT_EQ(getConfig().getUserIgnoreFile(), "/var/user/42/myignore");
}
