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

  /** Get the path to client certificate. */
  const std::optional<AbsolutePath> getClientCertificate() const;

  std::optional<std::string> getMononokeHostName() const;

  void setUserConfigPath(AbsolutePath userConfigPath);
  void setSystemConfigDir(AbsolutePath systemConfigDir);
  void setSystemConfigPath(AbsolutePath systemConfigDir);

  /**
   * Clear all configuration for the given config source.
   */
  void clearAll(ConfigSource);

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

  /**
   * Mapping of section name : (map of attribute : config values). The
   * ConfigSetting constructor registration populates this map.
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

  /*
   * Settings follow. Their initialization registers themselves with the
   * EdenConfig object. We make use of the registration to iterate over
   * ConfigSettings generically for parsing, copy, assignment and move
   * operations. We update the property value in the constructor (since we don't
   * have the home directory here).
   *
   * The following fields must come after configMap_.
   */

 public:
  ConfigSetting<AbsolutePath> edenDir{"core:edenDirectory",
                                      kUnspecifiedDefault,
                                      this};

  ConfigSetting<AbsolutePath> systemIgnoreFile{"core:systemIgnoreFile",
                                               kUnspecifiedDefault,
                                               this};

  ConfigSetting<AbsolutePath> userIgnoreFile{"core:ignoreFile",
                                             kUnspecifiedDefault,
                                             this};

  ConfigSetting<bool> allowUnixGroupRequests{"thrift:allow-unix-group-requests",
                                             false,
                                             this};

  ConfigSetting<AbsolutePath> clientCertificate{"ssl:client-certificate",
                                                kUnspecifiedDefault,
                                                this};
  ConfigSetting<bool> useMononoke{"mononoke:use-mononoke", false, this};

  /**
   * Which tier to use when talking to mononoke.
   */
  ConfigSetting<std::string> mononokeTierName{"mononoke:tier",
                                              "mononoke-apiserver",
                                              this};
  ConfigSetting<std::string> mononokeHostName{"mononoke:hostname", "", this};
  ConfigSetting<uint16_t> mononokePort{"mononoke:port", 443, this};
  ConfigSetting<std::string> mononokeConnectionType{"mononoke:connection-type",
                                                    "http",
                                                    this};

  /**
   * How often the on-disk config information should be checked for changes.
   */
  ConfigSetting<std::chrono::nanoseconds> configReloadInterval{
      "config:reload-interval",
      std::chrono::minutes(5),
      this};

  /**
   * How often to compute stats and perform garbage collection management
   * for the LocalStore.
   */
  ConfigSetting<std::chrono::nanoseconds> localStoreManagementInterval{
      "store:stats-interval",
      std::chrono::minutes(1),
      this};

  /**
   * The size limit for ephemeral sections of the local store.
   *
   * Automatic garbage collection will be triggered when the size exceeds this
   * threshold.
   */
  ConfigSetting<size_t> localStoreEphemeralSizeLimit{
      "store:ephemeral-size-limit",
      20'000'000'000,
      this};

  /**
   * The maximum time duration allowed for a fuse request. If a request exceeds
   * this amount of time, an ETIMEDOUT error will be returned to the kernel to
   * avoid blocking forever.
   */
  ConfigSetting<std::chrono::nanoseconds> fuseRequestTimeout{
      "fuse:request-timeout",
      std::chrono::minutes(1),
      this};

  /**
   * The maximum time duration that the kernel should allow for a fuse request.
   * If a request exceeds this amount of time, it may take aggressive
   * measures to shut down the fuse channel.
   * This value is only applicable to the macOS fuse implementation.
   */
  ConfigSetting<std::chrono::nanoseconds> fuseDaemonTimeout{
      "fuse:daemon-timeout",
      std::chrono::nanoseconds::max(),
      this};

  /**
   * Controls whether Eden reads from Mercurial's datapack store.
   */
  ConfigSetting<bool> useDatapack{"hg:use-datapack", false, this};
};
} // namespace eden
} // namespace facebook
