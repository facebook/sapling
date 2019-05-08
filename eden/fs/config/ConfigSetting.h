/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <array>
#include <cstddef>
#include <map>
#include <optional>
#include <string>

#include <folly/Range.h>

#include "eden/fs/config/FieldConverter.h"

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
    return c(stringValue, attrMap).then([&](T&& convertResult) {
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
        configValueArray_[source].has_value()) {
      configValueArray_[source].reset();
    }
  }

  virtual ~ConfigSetting() {}

 private:
  /**
   * Stores the values, indexed by ConfigSource (as int). Optional is used to
   * allow unpopulated entries. Default values should always be present.
   */
  std::array<std::optional<T>, kConfigSourceLastIndex + 1> configValueArray_;
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

} // namespace eden
} // namespace facebook
