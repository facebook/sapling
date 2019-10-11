/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <optional>

#include <folly/dynamic.h>
#include <folly/portability/SysStat.h>
#include <folly/portability/SysTypes.h>
#include <folly/portability/Unistd.h>

#include "eden/fs/config/ConfigSetting.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ParentCommits.h"
#include "eden/fs/utils/PathFuncs.h"
#ifdef _WIN32
#include "eden/fs/win/utils/Stub.h" // @manual
#endif

extern const facebook::eden::RelativePathPiece kDefaultEdenDirectory;
extern const facebook::eden::RelativePathPiece kDefaultIgnoreFile;
extern const facebook::eden::AbsolutePath kUnspecifiedDefault;

namespace facebook {
namespace eden {

/**
 * EdenConfig holds the Eden configuration settings. It is constructed from
 * cli settings, user configuration files, system configuration files and
 * default values. It provides methods to determine if configuration files
 * have been changed (fstat on the source files).
 *
 * To augment configuration, add a ConfigurationSetting member variable to
 * this class. ConfigurationSettings require a key to identify the setting
 * in configuration files. For example, "core:edenDirectory".
 */
class EdenConfig : private ConfigSettingManager {
 public:
  /**
   * Manually construct a EdenConfig object. Users can subsequently use the
   * load methods to populate the EdenConfig.
   */
  explicit EdenConfig(
      folly::StringPiece userName,
      uid_t userID,
      AbsolutePath userHomePath,
      AbsolutePath userConfigPath,
      AbsolutePath systemConfigDir,
      AbsolutePath systemConfigPath);

  explicit EdenConfig(const EdenConfig& source);

  explicit EdenConfig(EdenConfig&& source) = delete;

  EdenConfig& operator=(const EdenConfig& source);

  EdenConfig& operator=(EdenConfig&& source) = delete;

  /**
   * Update EdenConfig by loading the system configuration.
   */
  void loadSystemConfig();

  /**
   * Update EdenConfig by loading the user configuration.
   */
  void loadUserConfig();

  /**
   * Load the configuration based on the passed path. The configuation source
   * identifies whether the config file is a system or user config file and
   * apply setting over-rides appropriately. The passed configFile stat is
   * updated with the config files fstat results.
   */
  void loadConfig(
      AbsolutePathPiece path,
      ConfigSource configSource,
      struct stat* configFileStat);

  /**
   * Stringify the EdenConfig for logging or debugging.
   */
  std::string toString() const;

  /**
   * Return the config data as a EdenConfigData structure that can be
   * thrift-serialized.
   */
  EdenConfigData toThriftConfigData() const;

  /** Determine if user config has changed, fstat userConfigFile.*/
  bool hasUserConfigFileChanged() const;

  /** Determine if user config has changed, fstat systemConfigFile.*/
  bool hasSystemConfigFileChanged() const;

  /** Get the user config path. Default "userHomePath/.edenrc" */
  const AbsolutePath& getUserConfigPath() const;

  /** Get the system config dir. Default "/etc/eden" */
  const AbsolutePath& getSystemConfigDir() const;

  /** Get the system config path. Default "/etc/eden/edenfs.rc" */
  const AbsolutePath& getSystemConfigPath() const;

  /** Get the system ignore file. Default "userHomePath/.eden" */
  const AbsolutePath& getSystemIgnoreFile() const;

  /** Get the eden directory. Default "/etc/eden/edenfs.rc" */
  const AbsolutePath& getEdenDir() const;

  /** Get the user ignore file. Default "userHomePath/ignore" */
  const AbsolutePath& getUserIgnoreFile() const;

  /** Get the path to client certificate. */
  const std::optional<AbsolutePath> getClientCertificate() const;

  /** Get the use mononoke flag. Default false */
  bool getUseMononoke() const;

  /** Which tier to use when talking to mononoke */
  const std::string& getMononokeTierName() const;
  std::optional<std::string> getMononokeHostName() const;
  uint16_t getMononokePort() const;

  /** Type of connection used to talk to Mononoke */
  const std::string& getMononokeConnectionType() const;

  /**
   * Get how often the on-disk config information should be checked for changes.
   */
  std::chrono::nanoseconds getConfigReloadInterval() const {
    return configReloadInterval_.getValue();
  }

  /**
   * Get how often to compute stats and perform garbage collection management
   * for the LocalStore.
   */
  std::chrono::nanoseconds getLocalStoreManagementInterval() const {
    return localStoreManagementInterval_.getValue();
  }

  /**
   * Get the size limit for ephemeral sections of the local store.
   *
   * Automatic garbage collection will be triggered when the size exceeds this
   * threshold.
   */
  size_t getLocalStoreEphemeralSizeLimit() const {
    return localStoreEphemeralSizeLimit_.getValue();
  }

  /**
   * Get the maximum time duration allowed for a fuse request.
   * If a request exceeds this amount of time, an ETIMEDOUT
   * error will be returned to the kernel to avoid blocking forever.
   */
  std::chrono::nanoseconds getFuseRequestTimeout() const {
    return fuseRequestTimeout_.getValue();
  }

  /**
   * Get the maximum time duration that the kernel should allow
   * for a fuse request.
   * If a request exceeds this amount of time, it may take aggressive
   * measures to shut down the fuse channel.
   * This value is only applicable to the macOS fuse implementation.
   */
  std::chrono::nanoseconds getFuseDaemonTimeout() const {
    return fuseDaemonTimeout_.getValue();
  }

  /**
   * Controls if Eden reads from Mercurial's datapack store.
   */
  bool getUseDatapack() const {
    return useDatapack_.getValue();
  }

  void setUserConfigPath(AbsolutePath userConfigPath);

  void setSystemConfigDir(AbsolutePath systemConfigDir);

  void setSystemConfigPath(AbsolutePath systemConfigDir);

  /**
   * Clear all configuration for the given config source.
   */
  void clearAll(ConfigSource);

  /** Set the system ignore file for the provided source.
   */
  void setSystemIgnoreFile(
      AbsolutePath systemIgnoreFile,
      ConfigSource configSource);

  /** Set the Eden directory for the provided source.
   */
  void setEdenDir(AbsolutePath edenDir, ConfigSource configSource);

  /** Set the ignore file for the provided source.
   */
  void setUserIgnoreFile(
      AbsolutePath userIgnoreFile,
      ConfigSource configSource);

  /** Set the client certificate file for the provided source.
   */
  void setClientCertificate(
      AbsolutePath clientCertificate,
      ConfigSource configSource);

  /** Set the use mononoke flag for the provided source.
   */
  void setUseMononoke(bool useMononoke, ConfigSource configSource);

  /**
   *  Register the configuration setting. The fullKey is used to parse values
   *  from the toml file. It is of the form: "core:userConfigPath"
   */
  void registerConfiguration(ConfigSettingBase* configSetting) override;

  /**
   * Returns the user's home directory
   */
  AbsolutePathPiece getUserHomePath() const;

  /**
   * Returns the user's username
   */
  const std::string& getUserName() const;

  /**
   * Returns the user's UID
   */
  uid_t getUserID() const;

 private:
  /**
   * Utility method for converting ConfigSource to the filename (or cli).
   * @return the string value for the ConfigSource.
   */
  std::string toString(facebook::eden::ConfigSource cs) const;

  void doCopy(const EdenConfig& source);

  void initConfigMap();

  void parseAndApplyConfigFile(
      int configFd,
      AbsolutePathPiece configPath,
      ConfigSource configSource);

  /** Mapping of section name : (map of attribute : config values). The
   *  ConfigSetting constructor registration populates this map.
   */
  std::map<std::string, std::map<std::string, ConfigSettingBase*>> configMap_;

  std::string userName_;
  uid_t userID_;
  AbsolutePath userHomePath_;
  AbsolutePath userConfigPath_;
  AbsolutePath systemConfigPath_;
  AbsolutePath systemConfigDir_;

  struct stat systemConfigFileStat_ = {};
  struct stat userConfigFileStat_ = {};

  /** Initialization registers ConfigSetting with the EdenConfig object.
   * We make use of the registration to iterate over ConfigSettings generically
   * for parsing, copy, assingnment and move operations.
   * We update the property value in the constructor (since we don't have the
   * home directory here).
   */
  ConfigSetting<AbsolutePath> edenDir_{"core:edenDirectory",
                                       kUnspecifiedDefault,
                                       this};

  ConfigSetting<AbsolutePath> systemIgnoreFile_{"core:systemIgnoreFile",
                                                kUnspecifiedDefault,
                                                this};

  ConfigSetting<AbsolutePath> userIgnoreFile_{"core:ignoreFile",
                                              kUnspecifiedDefault,
                                              this};

  ConfigSetting<AbsolutePath> clientCertificate_{"ssl:client-certificate",
                                                 kUnspecifiedDefault,
                                                 this};
  ConfigSetting<bool> useMononoke_{"mononoke:use-mononoke", false, this};
  ConfigSetting<std::string> mononokeTierName_{"mononoke:tier",
                                               "mononoke-apiserver",
                                               this};
  ConfigSetting<std::string> mononokeHostName_{"mononoke:hostname", "", this};
  ConfigSetting<uint16_t> mononokePort_{"mononoke:port", 443, this};
  ConfigSetting<std::string> mononokeConnectionType_{"mononoke:connection-type",
                                                     "http",
                                                     this};

  ConfigSetting<std::chrono::nanoseconds> configReloadInterval_{
      "config:reload-interval",
      std::chrono::minutes(5),
      this};
  ConfigSetting<std::chrono::nanoseconds> localStoreManagementInterval_{
      "store:stats-interval",
      std::chrono::minutes(1),
      this};
  ConfigSetting<size_t> localStoreEphemeralSizeLimit_{
      "store:ephemeral-size-limit",
      20'000'000'000,
      this};

  ConfigSetting<std::chrono::nanoseconds> fuseRequestTimeout_{
      "fuse:request-timeout",
      std::chrono::minutes(1),
      this};
  ConfigSetting<std::chrono::nanoseconds> fuseDaemonTimeout_{
      "fuse:daemon-timeout",
      std::chrono::nanoseconds::max(),
      this};

  ConfigSetting<bool> useDatapack_{"hg:use-datapack", false, this};
};
} // namespace eden
} // namespace facebook
