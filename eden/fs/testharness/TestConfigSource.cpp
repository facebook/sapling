/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/TestConfigSource.h"

#include <folly/MapUtil.h>

#include "eden/fs/config/ReloadableConfig.h"

namespace facebook::eden {

TestConfigSource::TestConfigSource(ConfigSourceType sourceType)
    : sourceType_{sourceType} {}

void TestConfigSource::setValues(Values values) {
  auto state = state_.wlock();
  state->values = std::move(values);
  state->shouldReload = true;
}

// ConfigSource methods:

ConfigSourceType TestConfigSource::getSourceType() {
  return sourceType_;
}
std::string TestConfigSource::getSourcePath() {
  return "test";
}
FileChangeReason TestConfigSource::shouldReload() {
  return state_.rlock()->shouldReload ? FileChangeReason::MTIME
                                      : FileChangeReason::NONE;
}
void TestConfigSource::reload(
    const ConfigVariables& substitutions,
    ConfigSettingMap& map) {
  auto state = state_.rlock();
  for (const auto& [sectionName, section] : state->values) {
    auto* configMapEntry = folly::get_ptr(map, sectionName);
    XCHECK(configMapEntry) << "EdenConfig does not have section named "
                           << sectionName;

    for (const auto& [entryKey, entryValue] : section) {
      auto* configMapKeyEntry = folly::get_ptr(*configMapEntry, entryKey);
      XCHECK(configMapKeyEntry) << "EdenConfig does not have setting named "
                                << sectionName << ":" << entryKey;
      auto rslt = (*configMapKeyEntry)
                      ->setStringValue(entryValue, substitutions, sourceType_);
      XCHECK(rslt) << "invalid config value for " << sectionName << ":"
                   << entryKey << " = " << entryValue << ", " << rslt.error();
    }
  }
}

namespace {
std::pair<std::string_view, std::string_view> splitKey(
    std::string_view keypair) {
  auto idx = keypair.find(':');
  if (idx == std::string_view::npos) {
    throwf<std::domain_error>("config name {} must have a colon", keypair);
  }
  return {keypair.substr(0, idx), keypair.substr(idx + 1)};
}
} // namespace

void updateTestEdenConfig(
    std::shared_ptr<TestConfigSource>& configSource,
    const std::shared_ptr<ReloadableConfig>& reloadableConfig,
    const std::map<std::string, std::string>& values) {
  std::map<std::string, std::map<std::string, std::string>> nested;

  for (auto& [key, value] : values) {
    auto [sectionName, configName] = splitKey(key);
    nested[std::string(sectionName)][std::string(configName)] = value;
  }

  configSource->setValues(nested);
  (void)reloadableConfig->getEdenConfig(ConfigReloadBehavior::ForceReload);
}
} // namespace facebook::eden
