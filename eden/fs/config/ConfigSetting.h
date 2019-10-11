/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <array>
#include <cstddef>
#include <map>
#include <optional>
#include <string>
#include <typeindex>

#include <folly/Range.h>

#include "eden/fs/config/FieldConverter.h"
#include "eden/fs/config/gen-cpp2/eden_config_types.h"

namespace facebook {
namespace eden {

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
  ConfigSettingBase(
      folly::StringPiece key,
      const std::type_info& valueType,
      ConfigSettingManager* csm)
      : key_{key}, valueType_{valueType} {
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
  const std::string& getConfigKey() const {
    return key_;
  }
  std::type_index getValueType() const {
    return valueType_;
  }

 protected:
  std::string key_;
  std::type_index valueType_;
};

/**
 * A Configuration setting is a piece of application configuration that can be
 * constructed by parsing a string. It retains values for various ConfigSources:
 * cli, user config, system config, and default. Access methods will return
 * values for the highest priority source.
 */
template <typename T, typename Converter = FieldConverter<T>>
class ConfigSetting final : private ConfigSettingBase {
  static_assert(!std::is_reference_v<T>);

 public:
  ConfigSetting(
      folly::StringPiece key,
      T value,
      ConfigSettingManager* configSettingManager)
      : ConfigSettingBase{key, typeid(T), configSettingManager} {
    getSlot(ConfigSource::Default).emplace(std::move(value));
  }

  ConfigSetting(const ConfigSetting&) = delete;
  ConfigSetting(ConfigSetting&&) = delete;

  /**
   * Delete the assignment operator. We support copying via 'copyFrom'.
   */
  ConfigSetting<T>& operator=(const ConfigSetting& rhs) = delete;
  ConfigSetting<T>& operator=(ConfigSetting&& rhs) = delete;

  /** Get the highest priority ConfigSource (we ignore unpopulated values).*/
  ConfigSource getSource() const override {
    return static_cast<ConfigSource>(getHighestPriorityIdx());
  }

  /** Get the highest priority value for this setting.*/
  const T& getValue() const {
    return getSlot(getSource()).value();
  }

  /** Get the string value for this setting. Intended for debug purposes. .*/
  std::string getStringValue() const override {
    return Converter{}.toDebugString(getValue());
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
    if (newSource == ConfigSource::Default) {
      return folly::makeUnexpected<std::string>(
          "Convert ignored for default value");
    }
    Converter c;
    return c.fromString(stringValue, attrMap).then([&](T&& convertResult) {
      getSlot(newSource).emplace(std::move(convertResult));
    });
  }

  /**
   * Set the value with the identified source.
   */
  void setValue(T newVal, ConfigSource newSource, bool force = false) {
    if (force || newSource != ConfigSource::Default) {
      getSlot(newSource).emplace(std::move(newVal));
    }
  }

  /** Clear the value for the passed ConfigSource. The operation will be
   * ignored for ConfigSource::Default. */
  void clearValue(ConfigSource source) override {
    if (source != ConfigSource::Default && getSlot(source).has_value()) {
      getSlot(source).reset();
    }
  }

  using ConfigSettingBase::getConfigKey;

  /// Not a public API, but used in tests.
  void copyFrom(const ConfigSetting& other) {
    const ConfigSettingBase& base = other;
    return copyFrom(base);
  }

  virtual ~ConfigSetting() {}

 private:
  /**
   * Support copying of ConfigSetting. We limit this to instance of
   * ConfigSetting.
   */
  void copyFrom(const ConfigSettingBase& rhs) override {
    // Normally, dynamic_cast would be a better fit here, but private
    // inheritance prevents its use. Instead, compare the value's typeid.
    if (getValueType() != rhs.getValueType()) {
      throw std::logic_error{folly::to<std::string>(
          "ConfigSetting<",
          getValueType().name(),
          "> copyFrom unknown type: ",
          rhs.getValueType().name())};
    }
    auto* rhsConfigSetting = static_cast<const ConfigSetting*>(&rhs);
    key_ = rhsConfigSetting->key_;
    configValueArray_ = rhsConfigSetting->configValueArray_;
  }

  static constexpr size_t kConfigSourceLastIndex =
      static_cast<size_t>(apache::thrift::TEnumTraits<ConfigSource>::max());

  std::optional<T>& getSlot(ConfigSource source) {
    return configValueArray_[static_cast<size_t>(source)];
  }
  const std::optional<T>& getSlot(ConfigSource source) const {
    return configValueArray_[static_cast<size_t>(source)];
  }

  /**
   *  Get the index of the highest priority source that is populated.
   */
  size_t getHighestPriorityIdx() const {
    for (auto idx = kConfigSourceLastIndex;
         idx > static_cast<size_t>(ConfigSource::Default);
         --idx) {
      if (configValueArray_[idx].has_value()) {
        return idx;
      }
    }
    return static_cast<size_t>(ConfigSource::Default);
  }

  /**
   * Stores the values, indexed by ConfigSource (as int). Optional is used to
   * allow unpopulated entries. Default values should always be present.
   */
  std::array<std::optional<T>, kConfigSourceLastIndex + 1> configValueArray_;
};

} // namespace eden
} // namespace facebook
