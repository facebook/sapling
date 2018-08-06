/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Optional.h>
#include <folly/dynamic.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <bitset>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ParentCommits.h"
#include "eden/fs/utils/PathFuncs.h"

extern const facebook::eden::RelativePathPiece kDefaultEdenDirectory;
extern const facebook::eden::RelativePathPiece kDefaultIgnoreFile;
extern const facebook::eden::AbsolutePath kUnspecifiedDefault;

namespace facebook {
namespace eden {

/**
 * ConfigSource identifies the point of origin of a config setting.
 * It is ordered from low to high precedence. Higher precedence
 * configuration values over-ride lower precedence values. A config
 * setting of COMMAND_LINE takes precedence over all other settings.
 * NOTE: ConfigSource enum values are used to access array elements. Thus,
 * they should be ordered from 0 to kConfigSourceLastIndex, with increments
 * of 1.
 */
enum ConfigSource {
  DEFAULT = 0,
  SYSTEM_CONFIG_FILE = 1,
  USER_CONFIG_FILE = 2,
  COMMAND_LINE = 3,
};
constexpr size_t kConfigSourceLastIndex = 3;
} // namespace eden
} // namespace facebook
namespace facebook {
namespace eden {

bool isValidAbsolutePath(folly::StringPiece path);

class ConfigSettingBase;

/**
 * ConfigSettingManager is an interface to allow ConfigSettings to be
 * registered. We use it to track all the ConfigSettings in EdenConfig. It
 * allows us to limit the steps involved in adding new settings.
 */
class ConfigSettingManager {
 public:
  virtual ~ConfigSettingManager() {}
  virtual void registerConfiguration(ConfigSettingBase* configSetting) = 0;
};

/**
 *  ConfigSettingBase defines an interface that allows us to treat
 *  configuration settings generically. A ConfigSetting can have multiple
 *  values, one for each configuration source. ConfigSettingBase provides
 *  accessors (setters/getters) that take/return string values. Subclasses,
 *  can provide type based accessors.
 */
class ConfigSettingBase {
 public:
  ConfigSettingBase(folly::StringPiece key, ConfigSettingManager* csm)
      : key_(key) {
    if (csm) {
      csm->registerConfiguration(this);
    }
  }

  ConfigSettingBase(const ConfigSettingBase& source) = default;

  ConfigSettingBase(ConfigSettingBase&& source) = default;

  /**
   * Delete the assignment operator. Our approach is to support in subclasses
   * via 'copyFrom'.
   */
  ConfigSettingBase& operator=(const ConfigSettingBase& rhs) = delete;

  /**
   * Allow sub-classes to selectively support a polymorphic copy operation.
   * This is slightly more clear than having a polymorphic assignment operator.
   */
  virtual void copyFrom(const ConfigSettingBase& rhs) = 0;

  virtual ~ConfigSettingBase() {}
  /**
   * Parse and set the value for the provided ConfigSource.
   * @return Optional will have error message if the value was invalid.
   */
  FOLLY_NODISCARD virtual folly::Expected<folly::Unit, std::string>
  setStringValue(
      folly::StringPiece stringValue,
      const std::map<std::string, std::string>& attrMap,
      ConfigSource newSource) = 0;
  /**
   * Get the ConfigSource of the configuration setting. It is the highest
   * priority ConfigurationSource of all populated values.
   */
  virtual ConfigSource getSource() const = 0;
  /**
   * Get a string representation of the configuration setting.
   */
  virtual std::string getStringValue() const = 0;
  /**
   * Clear the configuration value (if present) for the passed ConfigSource.
   */
  virtual void clearValue(ConfigSource source) = 0;
  /**
   * Get the configuration key (used to identify) this setting. They key is
   * used to identify the entry in a configuration file. Example "core.edenDir"
   */
  virtual const std::string& getConfigKey() const {
    return key_;
  }

 protected:
  std::string key_;
};

/**
 * Converters are used to convert strings into ConfigSettings. For example,
 * they are used to convert the string settings of configuration files.
 */
template <typename T>
class FieldConverter {};

template <>
class FieldConverter<AbsolutePath> {
 public:
  /**
   * Convert the passed string piece to an AbsolutePath.
   * @param convData is a map of conversion data that can be used by conversions
   * method (for example $HOME value.)
   * @return the converted AbsolutePath or an error message.
   */
  folly::Expected<AbsolutePath, std::string> operator()(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;
};

/**
 * A Configuration setting is a piece of application configuration that can be
 * constructed by parsing a string. It retains values for various ConfigSources:
 * cli, user config, system config, and default. Access methods will return
 * values for the highest priority source.
 */
template <typename T, typename Converter = FieldConverter<T>>
class ConfigSetting : public ConfigSettingBase {
 public:
  ConfigSetting(
      folly::StringPiece key,
      T value,
      ConfigSettingManager* configSettingManager)
      : ConfigSettingBase(key, configSettingManager) {
    configValueArray_[facebook::eden::DEFAULT].emplace(std::move(value));
  }

  /**
   * Delete the assignment operator. We support copying via 'copyFrom'.
   */
  ConfigSetting<T>& operator=(const ConfigSetting<T>& rhs) = delete;

  ConfigSetting<T>& operator=(const ConfigSetting<T>&& rhs) = delete;

  /**
   * Support copying of ConfigSetting. We limit this to instance of
   * ConfigSetting.
   */
  void copyFrom(const ConfigSettingBase& rhs) override {
    auto rhsConfigSetting = dynamic_cast<const ConfigSetting<T>*>(&rhs);
    if (!rhsConfigSetting) {
      throw std::runtime_error("ConfigSetting copyFrom unknown type");
    }
    key_ = rhsConfigSetting->key_;
    configValueArray_ = rhsConfigSetting->configValueArray_;
  }

  /** Get the highest priority ConfigSource (we ignore unpopulated values).*/
  ConfigSource getSource() const override {
    return (ConfigSource)getHighestPriorityIdx();
  }

  /** Get the highest priority value for this setting.*/
  const T& getValue() const {
    return configValueArray_[getHighestPriorityIdx()].value();
  }

  /** Get the string value for this setting. Intended for debug purposes. .*/
  std::string getStringValue() const override {
    return folly::to<std::string>(
        configValueArray_[getHighestPriorityIdx()].value());
  }

  /**
   * Set the value based on the passed string. The value is parsed using the
   * template's converter.
   * @return an error in the Optional if the operation failed.
   */
  folly::Expected<folly::Unit, std::string> setStringValue(
      folly::StringPiece stringValue,
      const std::map<std::string, std::string>& attrMap,
      ConfigSource newSource) override {
    if (newSource == facebook::eden::DEFAULT) {
      return folly::makeUnexpected<std::string>(
          "Convert ignored for default value");
    }
    Converter c;
    return c(stringValue, attrMap).then([&](AbsolutePath&& convertResult) {
      configValueArray_[newSource].emplace(std::move(convertResult));
    });
  }

  /**
   * Set the value with the identified source.
   */
  void setValue(T newVal, ConfigSource newSource, bool force = false) {
    if (force || newSource != facebook::eden::DEFAULT) {
      configValueArray_[newSource].emplace(std::move(newVal));
    }
  }

  /** Clear the value for the passed ConfigSource. The operation will be
   * ignored for ConfigSource.DEFAULT. */
  void clearValue(ConfigSource source) override {
    if (source != facebook::eden::DEFAULT &&
        configValueArray_[source].hasValue()) {
      configValueArray_[source].reset();
    }
  }

  virtual ~ConfigSetting() {}

 private:
  /**
   * Stores the values, indexed by ConfigSource (as int). Optional is used to
   * allow unpopulated entries. Default values should always be present.
   */
  std::array<folly::Optional<T>, kConfigSourceLastIndex + 1> configValueArray_;
  /**
   *  Get the index of the highest priority source that is populated.
   */
  size_t getHighestPriorityIdx() const {
    for (auto idx = kConfigSourceLastIndex; idx > facebook::eden::DEFAULT;
         --idx) {
      if (configValueArray_[idx].has_value()) {
        return idx;
      }
    }
    return facebook::eden::DEFAULT;
  }
};

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
class EdenConfig : public ConfigSettingManager {
 public:
  /**
   * Manually construct a EdenConfig object. Users can subsequently use the
   * load methods to populate the EdenConfig.
   */
  explicit EdenConfig(
      folly::StringPiece userName,
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

  /**
   *  Register the configuration setting. The fullKey is used to parse values
   *  from the toml file. It is of the form: "core:userConfigPath"
   */
  void registerConfiguration(ConfigSettingBase* configSetting) override;

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
  AbsolutePath userHomePath_;
  AbsolutePath userConfigPath_;
  AbsolutePath systemConfigPath_;
  AbsolutePath systemConfigDir_;

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

  struct stat systemConfigFileStat_ = {};
  struct stat userConfigFileStat_ = {};
};
} // namespace eden
} // namespace facebook
