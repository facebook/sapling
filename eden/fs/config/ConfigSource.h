/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/FileChangeMonitor.h"
#include "eden/fs/config/gen-cpp2/eden_config_types.h"

namespace facebook::eden {

class ConfigSettingBase;
class ConfigVariables;

// This is a little gross. EdenConfig exposes its internal data structure here
// so ConfigSource can apply values to each setting with
// ConfigSettingBase::setStringValue.
//
// An intermediate abstraction might make sense in the future.
using ConfigSettingMap =
    std::map<std::string, std::map<std::string, ConfigSettingBase*>>;

class ConfigSource {
 public:
  virtual ~ConfigSource() = default;

  /**
   * Returns the slot where this source lives in the config hierarchy.
   */
  virtual ConfigSourceType getSourceType() = 0;

  /**
   * Returns the path to the file or URL backing this source.
   * Returns an empty string if not relevant.
   */
  virtual std::string getSourcePath() = 0;

  /**
   * Has the backing data changed? Should reload() be called?
   */
  virtual FileChangeReason shouldReload() = 0;

  /**
   * Load and apply new values to the configuration `map`.
   */
  virtual void reload(
      const ConfigVariables& substitutions,
      ConfigSettingMap& map) = 0;
};

class NullConfigSource final : public ConfigSource {
 public:
  explicit NullConfigSource(ConfigSourceType sourceType)
      : sourceType_{sourceType} {}

  ConfigSourceType getSourceType() override {
    return sourceType_;
  }
  std::string getSourcePath() override {
    return "";
  }
  FileChangeReason shouldReload() override {
    return FileChangeReason::NONE;
  }
  void reload(
      const ConfigVariables& /*substitutions*/,
      ConfigSettingMap& /*map*/) override {}

 private:
  ConfigSourceType sourceType_;
};

} // namespace facebook::eden
