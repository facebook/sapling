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

#include "eden/fs/config/TomlFileConfigSource.h"
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
  AbsolutePath systemConfigDir_;
  AbsolutePath defaultUserConfigPath_;
  AbsolutePath defaultDynamicConfigPath_;
  AbsolutePath defaultSystemConfigPath_;

  // Used by various tests to verify default values is set
  AbsolutePath defaultUserIgnoreFilePath_;
  AbsolutePath defaultSystemIgnoreFilePath_;
  AbsolutePath defaultEdenDirPath_;
  RelativePath clientCertificatePath_{"home/bob/client.pem"};
  bool defaultUseMononoke_ = false;
  size_t defaultTreeCacheMinimumItems_ = 16;

  // Map of test names to system, dynamic, user path
  std::map<std::string, std::tuple<AbsolutePath, AbsolutePath, AbsolutePath>>
      testPathMap_;
  std::string simpleOverRideTest_{"simpleOverRideTest"};

  void SetUp() override {
    testHomeDir_ = canonicalPath("/home") + PathComponentPiece{testUser_};
    systemConfigDir_ = canonicalPath("/etc/eden");
    defaultUserConfigPath_ = testHomeDir_ + ".edenrc"_pc;
    defaultSystemConfigPath_ = systemConfigDir_ + "edenfs.rc"_pc;
    defaultDynamicConfigPath_ = systemConfigDir_ + "edenfs_dynamic.rc"_pc;

    defaultUserIgnoreFilePath_ = testHomeDir_ + ".edenignore"_pc;
    defaultSystemIgnoreFilePath_ = systemConfigDir_ + "ignore"_pc;
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

    auto dynamicConfigPath = systemConfigDir + "edenfs_dynamic.rc"_pc;
    auto dynamicConfigFileData = folly::StringPiece{
        "[treecache]\n"
        "minimum-items=\"32\""};
    writeFile(dynamicConfigPath, folly::StringPiece{dynamicConfigFileData})
        .value();

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
        std::tuple<AbsolutePath, AbsolutePath, AbsolutePath>(
            systemConfigPath, dynamicConfigPath, userConfigPath);
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
  auto edenConfig = std::make_shared<EdenConfig>(
      ConfigVariables{},
      testHomeDir_,
      systemConfigDir_,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              defaultSystemConfigPath_, ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              defaultDynamicConfigPath_, ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              defaultUserConfigPath_, ConfigSourceType::UserConfig)});

  // Configuration
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(edenConfig->useMononoke.getValue(), defaultUseMononoke_);
  EXPECT_EQ(
      edenConfig->inMemoryTreeCacheMinimumItems.getValue(),
      defaultTreeCacheMinimumItems_);
}

TEST_F(EdenConfigTest, simpleSetGetTest) {
  AbsolutePath userConfigPath =
      testHomeDir_ + "differentConfigPath/.edenrc"_relpath;
  AbsolutePath systemConfigPath = canonicalPath("/etc/eden/fix/edenfs.rc");
  AbsolutePath dynamicConfigPath =
      canonicalPath("/etc/eden/fix/edenfs_dynamic.rc");
  AbsolutePath systemConfigDir = canonicalPath("/etc/eden/fix");

  ConfigVariables substitutions;
  substitutions["USER"] = testUser_;

  auto edenConfig = std::make_shared<EdenConfig>(
      std::move(substitutions),
      testHomeDir_,
      systemConfigDir,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              systemConfigPath, ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              dynamicConfigPath, ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              userConfigPath, ConfigSourceType::UserConfig)});

  AbsolutePath ignoreFile = canonicalPath("/home/bob/alternativeIgnore");
  AbsolutePath systemIgnoreFile = canonicalPath("/etc/eden/fix/systemIgnore");
  AbsolutePath edenDir = canonicalPath("/home/bob/alt/.eden");
  AbsolutePath clientCertificate = rootTestDir_ + clientCertificatePath_;
  bool useMononoke = true;
  size_t treeCacheMinimumItems = 36;

  // Configuration
  edenConfig->userIgnoreFile.setValue(
      ignoreFile, ConfigSourceType::CommandLine);
  edenConfig->systemIgnoreFile.setValue(
      systemIgnoreFile, ConfigSourceType::CommandLine);
  edenConfig->edenDir.setValue(edenDir, ConfigSourceType::CommandLine);
  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate.asString()}, ConfigSourceType::CommandLine);
  edenConfig->useMononoke.setValue(useMononoke, ConfigSourceType::CommandLine);
  edenConfig->inMemoryTreeCacheMinimumItems.setValue(
      treeCacheMinimumItems, ConfigSourceType::CommandLine);

  // Configuration
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), ignoreFile);
  EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), systemIgnoreFile);
  EXPECT_EQ(edenConfig->edenDir.getValue(), edenDir);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate.asString()));
  EXPECT_EQ(edenConfig->useMononoke.getValue(), useMononoke);
  EXPECT_EQ(
      edenConfig->inMemoryTreeCacheMinimumItems.getValue(),
      treeCacheMinimumItems);
}

TEST_F(EdenConfigTest, cloneTest) {
  AbsolutePath ignoreFile = canonicalPath("/NON_DEFAULT_IGNORE_FILE");
  AbsolutePath systemIgnoreFile =
      canonicalPath("/NON_DEFAULT_SYSTEM_IGNORE_FILE");
  AbsolutePath edenDir = canonicalPath("/NON_DEFAULT_EDEN_DIR");
  AbsolutePath clientCertificate =
      rootTestDir_ + PathComponent{"NON_DEFAULT_CLIENT_CERTIFICATE"};
  writeFile(clientCertificate, folly::StringPiece{"test"}).value();
  bool useMononoke = true;
  size_t treeCacheMinimumItems = 36;

  ConfigVariables substitutions;
  substitutions["USER"] = testUser_;

  std::shared_ptr<EdenConfig> configCopy;
  {
    auto edenConfig = std::make_shared<EdenConfig>(
        std::move(substitutions),
        testHomeDir_,
        systemConfigDir_,
        EdenConfig::SourceVector{
            std::make_shared<TomlFileConfigSource>(
                defaultSystemConfigPath_, ConfigSourceType::SystemConfig),
            std::make_shared<TomlFileConfigSource>(
                defaultDynamicConfigPath_, ConfigSourceType::Dynamic),
            std::make_shared<TomlFileConfigSource>(
                defaultUserConfigPath_, ConfigSourceType::UserConfig)});

    // Configuration
    edenConfig->userIgnoreFile.setValue(
        ignoreFile, ConfigSourceType::CommandLine);
    edenConfig->systemIgnoreFile.setValue(
        systemIgnoreFile, ConfigSourceType::SystemConfig);
    edenConfig->edenDir.setValue(edenDir, ConfigSourceType::UserConfig);
    edenConfig->clientCertificateLocations.setValue(
        {clientCertificate.asString()}, ConfigSourceType::UserConfig);
    edenConfig->useMononoke.setValue(useMononoke, ConfigSourceType::UserConfig);
    edenConfig->inMemoryTreeCacheMinimumItems.setValue(
        treeCacheMinimumItems, ConfigSourceType::CommandLine);

    EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), ignoreFile);
    EXPECT_EQ(edenConfig->systemIgnoreFile.getValue(), systemIgnoreFile);
    EXPECT_EQ(edenConfig->edenDir.getValue(), edenDir);
    EXPECT_EQ(
        edenConfig->getClientCertificate(),
        normalizeBestEffort(clientCertificate.asString()));
    EXPECT_EQ(edenConfig->useMononoke.getValue(), useMononoke);
    EXPECT_EQ(
        edenConfig->inMemoryTreeCacheMinimumItems.getValue(),
        treeCacheMinimumItems);

    configCopy = std::make_shared<EdenConfig>(*edenConfig);
  }

  EXPECT_EQ(configCopy->userIgnoreFile.getValue(), ignoreFile);
  EXPECT_EQ(configCopy->systemIgnoreFile.getValue(), systemIgnoreFile);
  EXPECT_EQ(configCopy->edenDir.getValue(), edenDir);
  EXPECT_EQ(
      configCopy->getClientCertificate(),
      normalizeBestEffort(clientCertificate.asString()));
  EXPECT_EQ(configCopy->useMononoke.getValue(), useMononoke);
  EXPECT_EQ(
      configCopy->inMemoryTreeCacheMinimumItems.getValue(),
      treeCacheMinimumItems);

  configCopy->clearAll(ConfigSourceType::UserConfig);
  configCopy->clearAll(ConfigSourceType::Dynamic);
  configCopy->clearAll(ConfigSourceType::SystemConfig);
  configCopy->clearAll(ConfigSourceType::CommandLine);

  EXPECT_EQ(configCopy->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      configCopy->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(configCopy->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(configCopy->useMononoke.getValue(), defaultUseMononoke_);
  EXPECT_EQ(
      configCopy->inMemoryTreeCacheMinimumItems.getValue(),
      defaultTreeCacheMinimumItems_);
}

TEST_F(EdenConfigTest, clearAllTest) {
  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
      testHomeDir_,
      systemConfigDir_,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              defaultSystemConfigPath_, ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              defaultDynamicConfigPath_, ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              defaultUserConfigPath_, ConfigSourceType::UserConfig)});

  AbsolutePath fromUserConfigPath =
      defaultUserConfigPath_ + "FROM_USER_CONFIG"_pc;
  AbsolutePath fromSystemConfigPath =
      systemConfigDir_ + "FROM_SYSTEM_CONFIG"_pc;
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
  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
      testHomeDir_,
      systemConfigDir_,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              defaultSystemConfigPath_, ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              defaultDynamicConfigPath_, ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              defaultUserConfigPath_, ConfigSourceType::UserConfig)});

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

TEST_F(EdenConfigTest, loadSystemDynamicUserConfigTest) {
  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
      testHomeDir_,
      systemConfigDir_,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              std::get<0>(testPathMap_[simpleOverRideTest_]),
              ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              std::get<1>(testPathMap_[simpleOverRideTest_]),
              ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              std::get<2>(testPathMap_[simpleOverRideTest_]),
              ConfigSourceType::UserConfig)});

  auto clientConfigPath = rootTestDir_ + clientCertificatePath_;

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
  EXPECT_EQ(edenConfig->inMemoryTreeCacheMinimumItems.getValue(), 32);
}

TEST_F(EdenConfigTest, nonExistingConfigFiles) {
  auto userConfigPath = testHomeDir_ + ".FILE_DOES_NOT_EXIST"_pc;
  auto systemConfigPath = systemConfigDir_ + "FILE_DOES_NOT_EXIST.rc"_pc;
  auto dynamicConfigPath = systemConfigDir_ + "FILE_DOES_NOT_EXIST_cfgr.rc"_pc;

  auto edenConfig = std::make_shared<EdenConfig>(
      getDefaultVariables(),
      testHomeDir_,
      systemConfigDir_,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              systemConfigPath, ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              dynamicConfigPath, ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              userConfigPath, ConfigSourceType::UserConfig)});

  // Check default configuration is set
  EXPECT_EQ(edenConfig->userIgnoreFile.getValue(), defaultUserIgnoreFilePath_);
  EXPECT_EQ(
      edenConfig->systemIgnoreFile.getValue(), defaultSystemIgnoreFilePath_);
  EXPECT_EQ(edenConfig->edenDir.getValue(), defaultEdenDirPath_);
  EXPECT_EQ(edenConfig->useMononoke.getValue(), defaultUseMononoke_);
  EXPECT_EQ(
      edenConfig->inMemoryTreeCacheMinimumItems.getValue(),
      defaultTreeCacheMinimumItems_);
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

    return EdenConfig{
        std::move(substitutions),
        canonicalPath("/testhomedir"),
        systemConfigDir,
        EdenConfig::SourceVector{
            std::make_shared<TomlFileConfigSource>(
                systemConfigDir + "system-edenrc"_pc,
                ConfigSourceType::SystemConfig),
            std::make_shared<TomlFileConfigSource>(
                systemConfigDir + "edenfs_dynamic.rc"_pc,
                ConfigSourceType::Dynamic),
            std::make_shared<TomlFileConfigSource>(
                userConfigPath, ConfigSourceType::UserConfig)}};
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
  auto userConfigPath = userConfigDir + ".edenrc"_pc;

  TomlFileConfigSource source{userConfigPath, ConfigSourceType::UserConfig};
  EXPECT_EQ(FileChangeReason::NONE, source.shouldReload());
  // shouldReload updates its internal state, so check that it hasn't changed
  // its mind.
  EXPECT_EQ(FileChangeReason::NONE, source.shouldReload());
}

TEST_F(EdenConfigTest, clientCertIsFirstAvailable) {
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
      systemConfigDir_,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              defaultSystemConfigPath_, ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              defaultDynamicConfigPath_, ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              defaultUserConfigPath_, ConfigSourceType::UserConfig)});

  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate1.asString(), clientCertificate2.asString()},
      ConfigSourceType::UserConfig);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate1.asString()));

  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate2.asString(), clientCertificate1.asString()},
      ConfigSourceType::UserConfig);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate2.asString()));

  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate1.asString(), clientCertificate3.asString()},
      ConfigSourceType::UserConfig);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate1.asString()));

  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate3.asString(), clientCertificate1.asString()},
      ConfigSourceType::UserConfig);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate1.asString()));
  edenConfig->clientCertificateLocations.setValue(
      {"${A_NON_EXISTANT_ENV_VAR}", clientCertificate1.asString()},
      ConfigSourceType::UserConfig);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate1.asString()));
}

TEST_F(EdenConfigTest, fallbackToOldSingleCertConfig) {
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
      systemConfigDir_,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              defaultSystemConfigPath_, ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              defaultDynamicConfigPath_, ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              defaultUserConfigPath_, ConfigSourceType::UserConfig)});

  // Without clientCertificateLocations set clientCertificate should be used.
  edenConfig->clientCertificate.setValue(
      clientCertificate4, ConfigSourceType::UserConfig);
  edenConfig->clientCertificateLocations.setValue(
      {}, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate4.asString()));

  // Now that clientCertificateLocations is set this should be used.
  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate1.asString(), clientCertificate2.asString()},
      ConfigSourceType::UserConfig);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate1.asString()));

  // Now that clientCertificateLocations does not contain a valid cert we should
  // fall back to the old single cert.
  edenConfig->clientCertificateLocations.setValue(
      {clientCertificate3.asString()}, ConfigSourceType::UserConfig);
  EXPECT_EQ(
      edenConfig->getClientCertificate(),
      normalizeBestEffort(clientCertificate4.asString()));
}

TEST_F(EdenConfigTest, getValueByFullKey) {
  auto edenConfig = std::make_shared<EdenConfig>(
      ConfigVariables{},
      testHomeDir_,
      systemConfigDir_,
      EdenConfig::SourceVector{
          std::make_shared<TomlFileConfigSource>(
              defaultSystemConfigPath_, ConfigSourceType::SystemConfig),
          std::make_shared<TomlFileConfigSource>(
              defaultDynamicConfigPath_, ConfigSourceType::Dynamic),
          std::make_shared<TomlFileConfigSource>(
              defaultUserConfigPath_, ConfigSourceType::UserConfig)});

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
