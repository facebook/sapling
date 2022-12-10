/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/EdenConfig.h"

#include <boost/algorithm/string/replace.hpp>
#include <folly/Range.h>
#include <folly/experimental/TestUtil.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <optional>

#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/FileUtils.h"
#include "eden/fs/utils/PathFuncs.h"

using folly::test::TemporaryDirectory;
using namespace facebook::eden;
using namespace facebook::eden::path_literals;

namespace {

class EdenConfigTest : public ::testing::Test {
 protected:
  // Top level directory to hold test artifacts
  std::unique_ptr<TemporaryDirectory> rootTestTempDir_;
  AbsolutePath rootTestDir_;

  // Default paths for when the path does not have to exist
  std::string testUser_{"bob"};
  AbsolutePath testHomeDir_;
  AbsolutePath defaultUserConfigPath_;
  AbsolutePath defaultSystemConfigPath_;

  // Used by various tests to verify default values is set
  AbsolutePath defaultUserIgnoreFilePath_;
  AbsolutePath defaultSystemIgnoreFilePath_;
  AbsolutePath defaultEdenDirPath_;
  RelativePath clientCertificatePath_{"home/bob/client.pem"};
  std::optional<AbsolutePath> defaultClientCertificatePath_;
  bool defaultUseMononoke_ = false;

  // Map of test names to system, user path
  std::map<std::string, std::pair<AbsolutePath, AbsolutePath>> testPathMap_;
  std::string simpleOverRideTest_{"simpleOverRideTest"};

  void SetUp() override {
    testHomeDir_ = canonicalPath("/home") + PathComponentPiece{testUser_};
    defaultUserConfigPath_ = testHomeDir_ + ".edenrc"_pc;
    defaultSystemConfigPath_ = canonicalPath("/etc/eden/edenfs.rc");

    defaultUserIgnoreFilePath_ = testHomeDir_ + ".edenignore"_pc;
    defaultSystemIgnoreFilePath_ = canonicalPath("/etc/eden/ignore");
    defaultEdenDirPath_ = testHomeDir_ + ".eden"_pc;

    rootTestTempDir_ =
        std::make_unique<TemporaryDirectory>("eden_sys_user_config_test_");
    rootTestDir_ = canonicalPath(rootTestTempDir_->path().string());
    setupSimpleOverRideTest();
  }

  void TearDown() override {
    rootTestTempDir_.reset();
  }

  void setupSimpleOverRideTest() {
    // we need to create the config path, since our getConfig will check that
    // the config file exists before returning it.
    auto homePath = rootTestDir_ + "home"_pc;
    ensureDirectoryExists(homePath);
    auto userPath = homePath + "bob"_pc;
    ensureDirectoryExists(userPath);

    auto clientConfigPath = AbsolutePath(rootTestDir_ + clientCertificatePath_);
    writeFile(clientConfigPath, folly::StringPiece{"test"}).value();

    auto testCaseDir = rootTestDir_ + PathComponent(simpleOverRideTest_);
    ensureDirectoryExists(testCaseDir);

    auto userConfigDir = testCaseDir + "client"_pc;
    ensureDirectoryExists(userConfigDir);

    auto userConfigPath = userConfigDir + ".edenrc"_pc;
    auto userConfigFileData = folly::StringPiece{
        "[core]\n"
        "ignoreFile=\"${HOME}/${USER}/userCustomIgnore\"\n"
        "[mononoke]\n"
        "use-mononoke=\"false\""};
    writeFile(userConfigPath, userConfigFileData).value();

    auto systemConfigDir = testCaseDir + "etc-eden"_pc;
    ensureDirectoryExists(systemConfigDir);

    auto systemConfigPath = systemConfigDir + "edenfs.rc"_pc;
    auto systemConfigFileData = fmt::format(
        "[core]\n"
        "ignoreFile='{}'\n"
        "systemIgnoreFile='{}'\n"
        "[mononoke]\n"
        "use-mononoke=true\n"
        "[ssl]\n"
        "client-certificate-locations=['{}']\n",
        folly::kIsWindows ? "\\\\?\\should_be_over_ridden"
                          : "/should_be_over_ridden",
        folly::kIsWindows ? "\\\\?\\etc\\eden\\systemCustomIgnore"
                          : "/etc/eden/systemCustomIgnore",
        clientConfigPath);
    writeFile(systemConfigPath, folly::StringPiece{systemConfigFileData})
        .value();

    testPathMap_[simpleOverRideTest_] =
        std::pair<AbsolutePath, AbsolutePath>(systemConfigPath, userConfigPath);
  }

  ConfigVariables getDefaultVariables() {
    ConfigVariables rv;
    rv["HOME"] = testHomeDir_.c_str();
    rv["USER"] = testUser_;
    rv["USER_ID"] = "0";
    return rv;
  }
};
} // namespace

TEST_F(EdenConfigTest, defaultTest) {
  AbsolutePath systemConfigDir = canonicalPath("/etc/eden");

  auto edenConfig = std::make_shared<EdenConfig>(
      ConfigVariables{},
      testHomeDir_,
      defaultUserConfigPath_,
      systemConfigDir,
      defaultSystemConfigPath_);

  // Config path
  EXPECT_EQ(edenConfig->getUserConfigPath(), defaultUserConfigPath_);
  EXPECT_EQ(edenConfig->getSystemConfigPath(), defaultSystemConfigPath_);

  // Configuration
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(edenConfig->getClientCertificate(), defaultClientCertificatePath_);
  EXPECT_EQ(edenConfig->useMononoke.getValue(), defaultUseMononoke_);
}

TEST_F(EdenConfigTest, simpleSetGetTest) {
  AbsolutePath userConfigPath =
      testHomeDir_ + "differentConfigPath/.edenrc"_relpath;
  AbsolutePath systemConfigPath = canonicalPath("/etc/eden/fix/edenfs.rc");
  AbsolutePath systemConfigDir = canonicalPath("/etc/eden/fix");

  ConfigVariables substitutions;
  substitutions["USER"] = testUser_;

  auto edenConfig = std::make_shared<EdenConfig>(
      std::move(substitutions),
      testHomeDir_,
      userConfigPath,
      systemConfigDir,
      systemConfigPath);

  AbsolutePath ignoreFile = canonicalPath("/home/bob/alternativeIgnore");
  AbsolutePath systemIgnoreFile = canonicalPath("/etc/eden/fix/systemIgnore");
  AbsolutePath edenDir = canonicalPath("/home/bob/alt/.eden");
  AbsolutePath clientCertificate = rootTestDir_ + clientCertificatePath_;
  bool useMononoke = true;

  // Configuration
  edenConfig->userIgnoreFile.setValue(
      ignoreFile, ConfigSourceType::CommandLine);
  edenConfig->systemIgnoreFile.setValue(
      systemIgnoreFile, ConfigSourceType::CommandLine);
  edenConfig->edenDir.setValue(edenDir, ConfigSourceType::CommandLine);
  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate}, ConfigSourceType::CommandLine);
  edenConfig->useMononoke.setValue(useMononoke, ConfigSourceType::CommandLine);

  // Configuration
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), ignoreFile);
  EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), systemIgnoreFile);
  EXPECT_EQ(edenConfig->edenDir.getValue(), edenDir);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate);
  EXPECT_EQ(edenConfig->useMononoke.getValue(), useMononoke);
}

TEST_F(EdenConfigTest, cloneTest) {
  AbsolutePath systemConfigDir = canonicalPath("/etc/eden");

  AbsolutePath ignoreFile = canonicalPath("/NON_DEFAULT_IGNORE_FILE");
  AbsolutePath systemIgnoreFile =
      canonicalPath("/NON_DEFAULT_SYSTEM_IGNORE_FILE");
  AbsolutePath edenDir = canonicalPath("/NON_DEFAULT_EDEN_DIR");
  AbsolutePath clientCertificate =
      rootTestDir_ + PathComponent{"NON_DEFAULT_CLIENT_CERTIFICATE"};
  writeFile(clientCertificate, folly::StringPiece{"test"}).value();
  bool useMononoke = true;

  ConfigVariables substitutions;
  substitutions["USER"] = testUser_;

  std::shared_ptr<EdenConfig> configCopy;
  {
    auto edenConfig = std::make_shared<EdenConfig>(
        std::move(substitutions),
        testHomeDir_,
        defaultUserConfigPath_,
        systemConfigDir,
        defaultSystemConfigPath_);

    // Configuration
    edenConfig->userIgnoreFile.setValue(
        ignoreFile, ConfigSourceType::CommandLine);
    edenConfig->systemIgnoreFile.setValue(
        systemIgnoreFile, ConfigSourceType::SystemConfig);
    edenConfig->edenDir.setValue(edenDir, ConfigSourceType::UserConfig);
    edenConfig->clientCertificateLocations.setValue(
        {clientCertificate}, ConfigSourceType::UserConfig);
    edenConfig->useMononoke.setValue(useMononoke, ConfigSourceType::UserConfig);

    EXPECT_EQ(edenConfig->getUserConfigPath(), defaultUserConfigPath_);
    EXPECT_EQ(edenConfig->getSystemConfigPath(), defaultSystemConfigPath_);

    EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), ignoreFile);
    EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), systemIgnoreFile);
    EXPECT_EQ(edenConfig->edenDir.getValue(), edenDir);
    EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate);
    EXPECT_EQ(edenConfig->useMononoke.getValue(), useMononoke);

    configCopy = std::make_shared<EdenConfig>(*edenConfig);
  }

  EXPECT_EQ(configCopy->getUserConfigPath(), defaultUserConfigPath_);
  EXPECT_EQ(configCopy->getSystemConfigPath(), defaultSystemConfigPath_);

  EXPECT_EQ(configCopy->userIgnoreFile.getValue(), ignoreFile);
  EXPECT_EQ(configCopy->systemIgnoreFile.getValue(), systemIgnoreFile);
  EXPECT_EQ(configCopy->edenDir.getValue(), edenDir);
  EXPECT_EQ(configCopy->getClientCertificate(), clientCertificate);
  EXPECT_EQ(configCopy->useMononoke.getValue(), useMononoke);

  configCopy->clearAll(ConfigSourceType::UserConfig);
  configCopy->clearAll(ConfigSourceType::SystemConfig);
  configCopy->clearAll(ConfigSourceType::CommandLine);

  EXPECT_EQ(configCopy->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      configCopy->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(configCopy->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(configCopy->getClientCertificate(), defaultClientCertificatePath_);
  EXPECT_EQ(configCopy->useMononoke.getValue(), defaultUseMononoke_);
}

TEST_F(EdenConfigTest, clearAllTest) {
  AbsolutePath systemConfigDir = canonicalPath("/etc/eden");

  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
      testHomeDir_,
      defaultUserConfigPath_,
      systemConfigDir,
      defaultSystemConfigPath_);

  AbsolutePath fromUserConfigPath =
      defaultUserConfigPath_ + "FROM_USER_CONFIG"_pc;
  AbsolutePath fromSystemConfigPath = systemConfigDir + "FROM_SYSTEM_CONFIG"_pc;
  AbsolutePath fromCommandLine =
      defaultUserConfigPath_ + "alt/FROM_COMMAND_LINE"_relpath;

  // We will set the config on 3 properties, each with different sources
  // We will then run for each source to check results
  edenConfig->userIgnoreFile.setValue(
      fromUserConfigPath, ConfigSourceType::UserConfig);
  edenConfig->systemIgnoreFile.setValue(
      fromSystemConfigPath, ConfigSourceType::SystemConfig);
  edenConfig->edenDir.setValue(fromCommandLine, ConfigSourceType::CommandLine);
  edenConfig->edenDir.setValue(
      fromUserConfigPath, ConfigSourceType::UserConfig);
  edenConfig->edenDir.setValue(
      fromSystemConfigPath, ConfigSourceType::SystemConfig);

  // Check over-rides
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), fromUserConfigPath);
  EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), fromSystemConfigPath);
  EXPECT_EQ(edenConfig->edenDir.getValue(), fromCommandLine);

  // Clear UserConfig and check
  edenConfig->clearAll(ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), fromSystemConfigPath);
  EXPECT_EQ(edenConfig->edenDir.getValue(), fromCommandLine);

  // Clear SystemConfig and check
  edenConfig->clearAll(ConfigSourceType::SystemConfig);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), fromCommandLine);

  // Clear CommandLine and check
  edenConfig->clearAll(ConfigSourceType::CommandLine);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
}

TEST_F(EdenConfigTest, overRideNotAllowedTest) {
  AbsolutePath systemConfigDir = canonicalPath("/etc/eden");

  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
      testHomeDir_,
      defaultUserConfigPath_,
      systemConfigDir,
      defaultSystemConfigPath_);

  // Check default (starting point)
  EXPECT_EQ(
      edenConfig->userIgnoreFile.getValue(),
      canonicalPath("/home/bob/.edenignore"));

  // Set from cli and verify that cannot over-ride
  AbsolutePath cliIgnoreFile = canonicalPath("/CLI_IGNORE_FILE");
  AbsolutePath ignoreFile = canonicalPath("/USER_IGNORE_FILE");
  AbsolutePath systemIgnoreFile = canonicalPath("/SYSTEM_IGNORE_FILE");

  edenConfig->userIgnoreFile.setValue(
      cliIgnoreFile, ConfigSourceType::CommandLine);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), cliIgnoreFile);

  edenConfig->userIgnoreFile.setValue(
      cliIgnoreFile, ConfigSourceType::SystemConfig);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), cliIgnoreFile);

  edenConfig->userIgnoreFile.setValue(ignoreFile, ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), cliIgnoreFile);
}

TEST_F(EdenConfigTest, loadSystemUserConfigTest) {
  // TODO: GET THE BASE NAME FOR THE SYSTEM CONFIG DIR!
  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
      testHomeDir_,
      testPathMap_[simpleOverRideTest_].second,
      testPathMap_[simpleOverRideTest_].first,
      testPathMap_[simpleOverRideTest_].first);

  edenConfig->loadSystemConfig();

  auto clientConfigPath = rootTestDir_ + clientCertificatePath_;

  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(
      edenConfig->userIgnoreFile.getValue(),
      canonicalPath("/should_be_over_ridden"));
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(),
      canonicalPath("/etc/eden/systemCustomIgnore"));
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientConfigPath.view()));
  EXPECT_EQ(edenConfig->useMononoke.getValue(), true);

  edenConfig->loadUserConfig();

  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(
      edenConfig->userIgnoreFile.getValue(),
      canonicalPath("/home/bob/bob/userCustomIgnore"));
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(),
      canonicalPath("/etc/eden/systemCustomIgnore"));
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientConfigPath.view()));
  EXPECT_EQ(edenConfig->useMononoke.getValue(), false);
}

TEST_F(EdenConfigTest, nonExistingConfigFiles) {
  auto userConfigPath = testHomeDir_ + ".FILE_DOES_NOT_EXIST"_pc;
  auto systemConfigDir = canonicalPath("/etc/eden");
  auto systemConfigPath = systemConfigDir + "FILE_DOES_NOT_EXIST.rc"_pc;

  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
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
  auto systemConfigDir = rootTestDir_ + "etc-eden"_pc;
  ensureDirectoryExists(systemConfigDir);

  auto userConfigPath = rootTestDir_ + "user-edenrc"_pc;
  auto getConfig = [&]() {
    ConfigVariables substitutions;
    substitutions["HOME"] = canonicalPath("/testhomedir").c_str();
    substitutions["USER"] = "testusername";
    substitutions["USER_ID"] = "42";
    substitutions["THRIFT_TLS_CL_CERT_PATH"] = "edenTest";

    auto config = EdenConfig{
        std::move(substitutions),
        canonicalPath("/testhomedir"),
        userConfigPath,
        systemConfigDir,
        systemConfigDir + "system-edenrc"_pc};
    config.loadUserConfig();
    return EdenConfig{config};
  };

  writeFile(
      userConfigPath,
      folly::ByteRange{fmt::format(
          "[core]\n"
          "ignoreFile=\"{}\"\n",
          "${HOME}/myignore")})
      .value();
  EXPECT_EQ(
      getConfig().userIgnoreFile.getValue(),
      canonicalPath("/testhomedir/myignore"));

  writeFile(
      userConfigPath,
      folly::ByteRange{fmt::format(
          "[core]\n"
          "ignoreFile='{}'\n",
          folly::kIsWindows ? "\\\\?\\home\\${USER}\\myignore"
                            : "/home/${USER}/myignore")})
      .value();
  EXPECT_EQ(
      getConfig().userIgnoreFile.getValue(),
      canonicalPath("/home/testusername/myignore"));

  writeFile(
      userConfigPath,
      folly::ByteRange{fmt::format(
          "[core]\n"
          "ignoreFile='{}'\n",
          folly::kIsWindows ? "\\\\?\\var\\user\\${USER_ID}\\myignore"
                            : "/var/user/${USER_ID}/myignore")})
      .throwUnlessValue();
  EXPECT_EQ(
      getConfig().userIgnoreFile.getValue(),
      canonicalPath("/var/user/42/myignore"));

  writeFile(
      userConfigPath,
      folly::ByteRange{fmt::format(
          "[core]\n"
          "ignoreFile='{}'\n",
          folly::kIsWindows
              ? "\\\\?\\var\\user\\${THRIFT_TLS_CL_CERT_PATH}\\myignore"
              : "/var/user/${THRIFT_TLS_CL_CERT_PATH}/myignore")})
      .throwUnlessValue();
  EXPECT_EQ(
      getConfig().userIgnoreFile.getValue(),
      canonicalPath("/var/user/edenTest/myignore"));
}

TEST_F(EdenConfigTest, missing_config_files_never_change) {
  auto userConfigDir = rootTestDir_ + "user-home"_pc;
  auto systemConfigDir = rootTestDir_ + "etc-eden"_pc;
  auto userConfigPath = userConfigDir + ".edenrc"_pc;
  auto systemConfigPath = systemConfigDir + "edenrc.toml"_pc;

  ensureDirectoryExists(systemConfigDir);

  EdenConfig config{
      ConfigVariables{},
      userConfigDir,
      userConfigPath,
      systemConfigDir,
      systemConfigPath};
  config.loadUserConfig();
  EXPECT_EQ(FileChangeReason::NONE, config.hasUserConfigFileChanged().reason);
}

TEST_F(EdenConfigTest, clientCertIsFirstAvailable) {
  AbsolutePath systemConfigDir = canonicalPath("/etc/eden");

  // cert1 and cert2 are both be avialable, so they could be returned from
  // getConfig. However, cert3 is not available, so it can not be.
  AbsolutePath clientCertificate1 = rootTestDir_ + "cert1"_pc;
  writeFile(clientCertificate1, folly::StringPiece{"test"}).value();
  AbsolutePath clientCertificate2 = rootTestDir_ + "cert2"_pc;
  writeFile(clientCertificate2, folly::StringPiece{"test"}).value();
  AbsolutePath clientCertificate3 = rootTestDir_ + "cert3"_pc;

  auto edenConfig = std::make_shared<EdenConfig>(
      ConfigVariables{},
      testHomeDir_,
      defaultUserConfigPath_,
      systemConfigDir,
      defaultSystemConfigPath_);

  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate1, clientCertificate2}, ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate1);

  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate2, clientCertificate1}, ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate2);

  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate1, clientCertificate3}, ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate1);

  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate3, clientCertificate1}, ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate1);
}

TEST_F(EdenConfigTest, fallbackToOldSingleCertConfig) {
  AbsolutePath systemConfigDir = canonicalPath("/etc/eden");

  // used in list cert
  AbsolutePath clientCertificate1 = rootTestDir_ + "cert1"_pc;
  writeFile(clientCertificate1, folly::StringPiece{"test"}).value();
  AbsolutePath clientCertificate2 = rootTestDir_ + "cert2"_pc;
  writeFile(clientCertificate2, folly::StringPiece{"test"}).value();
  // used in invalid list cert
  AbsolutePath clientCertificate3 = rootTestDir_ + "cert3"_pc;
  // used in single cert
  AbsolutePath clientCertificate4 = rootTestDir_ + "cert4"_pc;

  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
      testHomeDir_,
      defaultUserConfigPath_,
      systemConfigDir,
      defaultSystemConfigPath_);

  // Without clientCertificateLocations set clientCertificate should be used.
  edenConfig->clientCertificate.setValue(
      clientCertificate4, ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate4);

  // Now that clientCertificateLocations is set this should be used.
  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate1, clientCertificate2}, ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate1);

  // Now that clientCertificateLocations does not contain a valid cert we should
  // fall back to the old single cert.
  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate3}, ConfigSourceType::UserConfig);
  EXPECT_EQ(edenConfig->getClientCertificate(), clientCertificate4);
}

TEST_F(EdenConfigTest, getValueByFullKey) {
  auto edenConfig = std::make_shared<EdenConfig>(
      ConfigVariables{},
      testHomeDir_,
      defaultUserConfigPath_,
      canonicalPath("/etc/eden"),
      defaultSystemConfigPath_);

  EXPECT_EQ(edenConfig->getValueByFullKey("mononoke:use-mononoke"), "false");
  edenConfig->useMononoke.setValue(true, ConfigSourceType::CommandLine);
  EXPECT_EQ(edenConfig->getValueByFullKey("mononoke:use-mononoke"), "true");

  EXPECT_EQ(
      edenConfig->getValueByFullKey("bad-section:use-mononoke"), std::nullopt);
  EXPECT_EQ(edenConfig->getValueByFullKey("mononoke:bad-entry"), std::nullopt);

  EdenBugDisabler noCrash;
  EXPECT_THROW_RE(
      edenConfig->getValueByFullKey("ill-formed-key"),
      std::runtime_error,
      "ill-formed");
}
