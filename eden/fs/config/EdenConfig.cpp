/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/EdenConfig.h"

#include <array>
#include <optional>

#include <boost/filesystem.hpp>
#include <folly/MapUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp/util/EnumUtils.h>

#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/EnumValue.h"

namespace facebook::eden {

namespace {

constexpr PathComponentPiece kDefaultUserIgnoreFile{".edenignore"};
constexpr PathComponentPiece kDefaultSystemIgnoreFile{"ignore"};
constexpr PathComponentPiece kDefaultEdenDirectory{".eden"};

std::pair<std::string_view, std::string_view> parseKey(
    std::string_view fullKey) {
  auto pos = fullKey.find(":");
  if (pos == std::string::npos) {
    EDEN_BUG() << "ConfigSetting key must contain a colon: " << fullKey;
  }

  std::string_view section{fullKey.data(), pos};
  std::string_view key = fullKey.substr(pos + 1);

  // Avoid use of locales. Standardize on - instead of _.
  auto isConfigChar = [](char c) {
    return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') ||
        (c >= 'A' && c <= 'Z') || c == '-';
  };

  for (char c : section) {
    if (!isConfigChar(c)) {
      EDEN_BUG() << "not a valid section name: " << fullKey;
    }
  }

  return {section, key};
}

} // namespace

const AbsolutePath kUnspecifiedDefault{};

// TODO: move this to TestMount.h or something.

std::shared_ptr<EdenConfig> EdenConfig::createTestEdenConfig() {
  ConfigVariables subst;
  subst["HOME"] = "/tmp";
  subst["USER"] = "testuser";
  subst["USER_ID"] = "0";

  return std::make_unique<EdenConfig>(
      std::move(subst),
      /*userHomePath=*/canonicalPath("/tmp"),
      /*systemConfigDir=*/canonicalPath("/tmp"),
      SourceVector{
          std::make_shared<NullConfigSource>(ConfigSourceType::SystemConfig),
          std::make_shared<NullConfigSource>(ConfigSourceType::UserConfig)});
}

std::string EdenConfig::toString(ConfigSourceType cs) const {
  switch (cs) {
    case ConfigSourceType::Default:
      return "default";
    case ConfigSourceType::SystemConfig:
    case ConfigSourceType::UserConfig:
      if (const auto& source = configSources_[folly::to_underlying(cs)]) {
        return source->getSourcePath();
      } else {
        return "";
      }
    case ConfigSourceType::CommandLine:
      return "command-line";
  }
  throwf<std::invalid_argument>(
      "invalid config source value: {}", enumValue(cs));
}

EdenConfigData EdenConfig::toThriftConfigData() const {
  EdenConfigData result;
  for (const auto& [sectionName, section] : configMap_) {
    for (const auto& [key, setting] : section) {
      auto keyName = fmt::format("{}:{}", sectionName, key);
      auto& configValue = result.values_ref()[keyName];
      configValue.parsedValue() = setting->getStringValue();
      configValue.sourceType() = setting->getSourceType();
      configValue.sourcePath() = toSourcePath(setting->getSourceType());
    }
  }
  return result;
}

std::string EdenConfig::toSourcePath(ConfigSourceType cs) const {
  if (const auto& source = configSources_[folly::to_underlying(cs)]) {
    return source->getSourcePath();
  } else {
    return {};
  }
}

EdenConfig::EdenConfig(
    ConfigVariables substitutions,
    AbsolutePathPiece userHomePath,
    AbsolutePathPiece systemConfigDir,
    SourceVector configSources)
    : substitutions_{
          std::make_shared<ConfigVariables>(std::move(substitutions))} {
  // Force set defaults that require passed arguments
  edenDir.setValue(
      userHomePath + kDefaultEdenDirectory, ConfigSourceType::Default, true);
  userIgnoreFile.setValue(
      userHomePath + kDefaultUserIgnoreFile, ConfigSourceType::Default, true);
  systemIgnoreFile.setValue(
      systemConfigDir + kDefaultSystemIgnoreFile,
      ConfigSourceType::Default,
      true);

  for (auto& source : configSources) {
    auto type = source->getSourceType();
    auto index = folly::to_underlying(type);
    XCHECK_NE(ConfigSourceType::Default, type)
        << "May not provide a ConfigSource of type Default. Default is prepopulated.";
    XCHECK(!configSources_[index])
        << "Multiple ConfigSources of the same type ("
        << apache::thrift::util::enumNameSafe(type) << ") are disallowed.";
    configSources_[index] = std::move(source);
  }

  reload();
}

EdenConfig::EdenConfig(const EdenConfig& source) {
  substitutions_ = source.substitutions_;
  configSources_ = source.configSources_;

  // Copy each ConfigSetting from source.
  for (const auto& [section, sectionMap] : source.configMap_) {
    for (const auto& [key, value] : sectionMap) {
      configMap_[section][key]->copyFrom(*value);
    }
  }
}

std::optional<std::string> EdenConfig::getValueByFullKey(
    std::string_view configKey) const {
  // Throws if the config key is ill-formed.
  auto [sectionKey, entryKey] = parseKey(configKey);

  if (auto* entry = folly::get_ptr(
          configMap_, std::string{sectionKey}, std::string{entryKey})) {
    return (*entry)->getStringValue();
  }

  return std::nullopt;
}

void EdenConfig::reload() {
  for (const auto& source : configSources_) {
    if (source) {
      source->reload(*substitutions_, configMap_);
    }
  }
}

std::shared_ptr<const EdenConfig> EdenConfig::maybeReload() const {
  std::shared_ptr<EdenConfig> newConfig;

  for (const auto& source : configSources_) {
    if (source) {
      if (auto reason = source->shouldReload()) {
        XLOGF(DBG3, "Reloading {} because {}", source->getSourcePath(), reason);

        if (!newConfig) {
          newConfig = std::make_shared<EdenConfig>(*this);
        }
        newConfig->clearAll(source->getSourceType());
        source->reload(*newConfig->substitutions_, newConfig->configMap_);
      }
    }
  }

  return newConfig;
}

void EdenConfig::registerConfiguration(ConfigSettingBase* configSetting) {
  std::string_view fullKeyStr = configSetting->getConfigKey();
  auto [section, key] = parseKey(fullKeyStr);

  auto& keyMap = configMap_[std::string{section}];
  keyMap[std::string{key}] = configSetting;
}

const std::optional<AbsolutePath> EdenConfig::getClientCertificate() const {
  // return the first cert path that exists
  for (auto& cert : clientCertificateLocations.getValue()) {
    if (boost::filesystem::exists(cert.asString())) {
      return cert;
    }
  }
  auto singleCertificateConfig = clientCertificate.getValue();
  if (singleCertificateConfig != kUnspecifiedDefault) {
    return singleCertificateConfig;
  }
  return std::nullopt;
}

void EdenConfig::clearAll(ConfigSourceType configSource) {
  for (const auto& sectionEntry : configMap_) {
    for (auto& keyEntry : sectionEntry.second) {
      keyEntry.second->clearValue(configSource);
    }
  }
}

} // namespace facebook::eden
