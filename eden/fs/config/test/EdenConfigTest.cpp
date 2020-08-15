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

#include "eden/fs/utils/FileUtils.h"
#include "eden/fs/utils/PathFuncs.h"

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
    auto testCaseDir = AbsolutePathPiece(rootTestDir_->path().string()) +
        PathComponent(simpleOverRideTest_);
    folly::fs::create_directory(folly::fs::path(testCaseDir.stringPiece()));

    auto userConfigDir = testCaseDir + "client"_pc;
    folly::fs::create_directory(folly::fs::path(userConfigDir.stringPiece()));

    auto userConfigPath = userConfigDir + ".edenrc"_pc;
    auto userConfigFileData = folly::StringPiece{
        "[core]\n"
        "ignoreFile=\"${HOME}/${USER}/userCustomIgnore\"\n"
        "[mononoke]\n"
        "use-mononoke=\"false\""};
    writeFile(userConfigPath, userConfigFileData).value();

    auto systemConfigDir = testCaseDir + "etc-eden"_pc;
    folly::fs::create_directory(folly::fs::path(systemConfigDir.stringPiece()));

    auto systemConfigPath = systemConfigDir + "edenfs.rc"_pc;
    auto systemConfigFileData = folly::StringPiece{
        "[core]\n"
        "ignoreFile=\"/should_be_over_ridden\"\n"
        "systemIgnoreFile=\"/etc/eden/systemCustomIgnore\"\n"
        "[mononoke]\n"
        "use-mononoke=true\n"
        "[ssl]\n"
        "client-certificate=\"/system_config_cert/${USER}/foo/${USER}\"\n"};
    writeFile(systemConfigPath, systemConfigFileData).value();

    testPathMap_[simpleOverRideTest_] =
        std::pair<AbsolutePath, AbsolutePath>(systemConfigPath, userConfigPath);
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
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(edenConfig->getClientCertificate(), defaultClientCertificatePath_);
  EXPECT_EQ(edenConfig->useMononoke.getValue(), defaultUseMononoke_);
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
  edenConfig->userIgnoreFile.setValue(ignoreFile, ConfigSource::CommandLine);
  edenConfig->systemIgnoreFile.setValue(
      systemIgnoreFile, ConfigSource::CommandLine);
  edenConfig->edenDir.setValue(edenDir, ConfigSource::CommandLine);
  edenConfig->clientCertificate.setValue(
      clientCertificate, ConfigSource::CommandLine);
  edenConfig->useMononoke.setValue(useMononoke, ConfigSource::CommandLine);

  // Config path
  EXPECT_EQ(edenConfig->getUserConfigPath(), updatedUserConfigPath);
  EXPECT_EQ(edenConfig->getSystemConfigDir(), updatedSystemConfigDir);
  EXPECT_EQ(edenConfig->getSystemConfigPath(), updatedSystemConfigPath);

  // Configuration
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), ignoreFile);
  EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), systemIgnoreFile);
  EXPECT_EQ(edenConfig->edenDir.getValue(), edenDir);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate);
  EXPECT_EQ(edenConfig->useMononoke.getValue(), useMononoke);
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
    edenConfig->userIgnoreFile.setValue(ignoreFile, ConfigSource::CommandLine);
    edenConfig->systemIgnoreFile.setValue(
        systemIgnoreFile, ConfigSource::SystemConfig);
    edenConfig->edenDir.setValue(edenDir, ConfigSource::UserConfig);
    edenConfig->clientCertificate.setValue(
        clientCertificate, ConfigSource::UserConfig);
    edenConfig->useMononoke.setValue(useMononoke, ConfigSource::UserConfig);

    EXPECT_EQ(edenConfig->getUserName(), testUser_);
    EXPECT_EQ(edenConfig->getUserID(), userID);
    EXPECT_EQ(edenConfig->getUserConfigPath(), userConfigPath);
    EXPECT_EQ(edenConfig->getSystemConfigPath(), systemConfigPath);
    EXPECT_EQ(edenConfig->getSystemConfigDir(), systemConfigDir);

    EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), ignoreFile);
    EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), systemIgnoreFile);
    EXPECT_EQ(edenConfig->edenDir.getValue(), edenDir);
    EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate);
    EXPECT_EQ(edenConfig->useMononoke.getValue(), useMononoke);

    configCopy = std::make_shared<EdenConfig>(*edenConfig);
  }

  EXPECT_EQ(configCopy->getUserName(), testUser_);
  EXPECT_EQ(configCopy->getUserID(), userID);
  EXPECT_EQ(configCopy->getUserConfigPath(), userConfigPath);
  EXPECT_EQ(configCopy->getSystemConfigPath(), systemConfigPath);
  EXPECT_EQ(configCopy->getSystemConfigDir(), systemConfigDir);

  EXPECT_EQ(configCopy->userIgnoreFile.getValue(), ignoreFile);
  EXPECT_EQ(configCopy->systemIgnoreFile.getValue(), systemIgnoreFile);
  EXPECT_EQ(configCopy->edenDir.getValue(), edenDir);
  EXPECT_EQ(configCopy->getClientCertificate(), clientCertificate);
  EXPECT_EQ(configCopy->useMononoke.getValue(), useMononoke);

  configCopy->clearAll(ConfigSource::UserConfig);
  configCopy->clearAll(ConfigSource::SystemConfig);
  configCopy->clearAll(ConfigSource::CommandLine);

  EXPECT_EQ(configCopy->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      configCopy->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(configCopy->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(configCopy->getClientCertificate(), defaultClientCertificatePath_);
  EXPECT_EQ(configCopy->useMononoke.getValue(), defaultUseMononoke_);
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
  edenConfig->userIgnoreFile.setValue(
      fromUserConfigPath, ConfigSource::UserConfig);
  edenConfig->systemIgnoreFile.setValue(
      fromSystemConfigPath, ConfigSource::SystemConfig);
  edenConfig->edenDir.setValue(fromCommandLine, ConfigSource::CommandLine);
  edenConfig->edenDir.setValue(fromUserConfigPath, ConfigSource::UserConfig);
  edenConfig->edenDir.setValue(
      fromSystemConfigPath, ConfigSource::SystemConfig);

  // Check over-rides
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), fromUserConfigPath);
  EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), fromSystemConfigPath);
  EXPECT_EQ(edenConfig->edenDir.getValue(), fromCommandLine);

  // Clear UserConfig and check
  edenConfig->clearAll(ConfigSource::UserConfig);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), fromSystemConfigPath);
  EXPECT_EQ(edenConfig->edenDir.getValue(), fromCommandLine);

  // Clear SystemConfig and check
  edenConfig->clearAll(ConfigSource::SystemConfig);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), fromCommandLine);

  // Clear CommandLine and check
  edenConfig->clearAll(ConfigSource::CommandLine);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
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
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), "/home/bob/.edenignore");

  // Set from cli and verify that cannot over-ride
  AbsolutePath cliIgnoreFile{"/CLI_IGNORE_FILE"};
  AbsolutePath ignoreFile{"/USER_IGNORE_FILE"};
  AbsolutePath systemIgnoreFile{"/SYSTEM_IGNORE_FILE"};

  edenConfig->userIgnoreFile.setValue(cliIgnoreFile, ConfigSource::CommandLine);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), cliIgnoreFile);

  edenConfig->userIgnoreFile.setValue(
      cliIgnoreFile, ConfigSource::SystemConfig);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), cliIgnoreFile);

  edenConfig->userIgnoreFile.setValue(ignoreFile, ConfigSource::UserConfig);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), cliIgnoreFile);
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

  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(
      edenConfig->userIgnoreFile.getValue(),
      normalizeBestEffort("/should_be_over_ridden"));
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(),
      normalizeBestEffort("/etc/eden/systemCustomIgnore"));
  EXPECT_EQ(
      edenConfig->getClientCertificate()->stringPiece(),
      normalizeBestEffort("/system_config_cert/bob/foo/bob"));
  EXPECT_EQ(edenConfig->useMononoke.getValue(), true);

  edenConfig->loadUserConfig();

  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(
      edenConfig->userIgnoreFile.getValue(),
      normalizeBestEffort("/home/bob/bob/userCustomIgnore"));
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(),
      normalizeBestEffort("/etc/eden/systemCustomIgnore"));
  EXPECT_EQ(
      edenConfig->getClientCertificate()->stringPiece(),
      normalizeBestEffort("/system_config_cert/bob/foo/bob"));
  EXPECT_EQ(edenConfig->useMononoke.getValue(), false);
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
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(edenConfig->getClientCertificate(), defaultClientCertificatePath_);
  EXPECT_EQ(edenConfig->useMononoke.getValue(), defaultUseMononoke_);
}

TEST_F(EdenConfigTest, variablesExpandInPathOptions) {
  auto rootTestDir = AbsolutePathPiece(rootTestDir_->path().string());
  auto systemConfigDir = rootTestDir + "etc-eden"_pc;
  folly::fs::create_directory(folly::fs::path(systemConfigDir.stringPiece()));

  auto userConfigPath = rootTestDir + "user-edenrc"_pc;
  auto getConfig = [&]() {
    auto config = EdenConfig{"testusername"_sp,
                             uid_t{42},
                             AbsolutePath("/testhomedir"),
                             userConfigPath,
                             systemConfigDir,
                             systemConfigDir + "system-edenrc"_pc};
    config.loadUserConfig();
    return EdenConfig{config};
  };

  writeFile(
      userConfigPath,
      "[core]\n"
      "ignoreFile=\"${HOME}/myignore\"\n"_sp)
      .value();
  EXPECT_EQ(
      getConfig().userIgnoreFile.getValue(),
      normalizeBestEffort("/testhomedir/myignore"));

  writeFile(
      userConfigPath,
      "[core]\n"
      "ignoreFile=\"/home/${USER}/myignore\"\n"_sp)
      .value();
  EXPECT_EQ(
      getConfig().userIgnoreFile.getValue(),
      normalizeBestEffort("/home/testusername/myignore"));

  writeFile(
      userConfigPath,
      "[core]\n"
      "ignoreFile=\"/var/user/${USER_ID}/myignore\"\n"_sp)
      .throwIfFailed();
  EXPECT_EQ(
      getConfig().userIgnoreFile.getValue(),
      normalizeBestEffort("/var/user/42/myignore"));
}
