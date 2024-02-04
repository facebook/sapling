/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <map>

#include <folly/Synchronized.h>

#include "eden/fs/config/EdenConfig.h"

namespace facebook::eden {

class ReloadableConfig;

class TestConfigSource final : public ConfigSource {
 public:
  using Values = std::map<std::string, std::map<std::string, std::string>>;

  explicit TestConfigSource(ConfigSourceType sourceType);

  void setValues(Values values);

  // ConfigSource methods:
  ConfigSourceType getSourceType() override;

  std::string getSourcePath() override;

  FileChangeReason shouldReload() override;

  void reload(const ConfigVariables& substitutions, ConfigSettingMap& map)
      override;

 private:
  ConfigSourceType sourceType_;
  struct State {
    bool shouldReload = false;
    Values values;
  };
  folly::Synchronized<State> state_;
};

void updateTestEdenConfig(
    std::shared_ptr<TestConfigSource>& configSource,
    const std::shared_ptr<ReloadableConfig>& reloadableConfig,
    const std::map<std::string, std::string>& values);

} // namespace facebook::eden
