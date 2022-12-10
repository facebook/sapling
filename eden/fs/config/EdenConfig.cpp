/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/EdenConfig.h"

#include <cpptoml.h>
#include <array>
#include <optional>

#include <boost/filesystem.hpp>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/MapUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>

#include "eden/fs/eden-config.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/EnumValue.h"

namespace facebook::eden {

namespace {

constexpr PathComponentPiece kDefaultUserIgnoreFile{".edenignore"};
constexpr PathComponentPiece kDefaultSystemIgnoreFile{"ignore"};
constexpr PathComponentPiece kDefaultEdenDirectory{".eden"};

void getConfigStat(
    AbsolutePathPiece configPath,
    int configFd,
    std::optional<FileStat>& configStat) {
  if (configFd >= 0) {
    auto result = getFileStat(configFd);
    // Report failure that is not due to ENOENT
    if (result.hasError()) {
      XLOG(WARN) << "error accessing config file " << configPath << ": "
                 << folly::errnoStr(result.error());
      configStat = std::nullopt;
    } else {
      configStat = result.value();
    }
  }
}

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

std::shared_ptr<EdenConfig> EdenConfig::createTestEdenConfig() {
  ConfigVariables subst;
  subst["HOME"] = "/tmp";
  subst["USER"] = "testuser";
  subst["USER_ID"] = "0";

  return std::make_unique<EdenConfig>(
      std::move(subst),
      /* userHomePath=*/canonicalPath("/tmp"),
      /* userConfigPath=*/canonicalPath("/tmp"),
      /* systemConfigDir=*/canonicalPath("/tmp"),
      /* setSystemConfigPath=*/canonicalPath("/tmp"));
}

std::string EdenConfig::toString(ConfigSourceType cs) const {
  switch (cs) {
    case ConfigSourceType::Default:
      return "default";
    case ConfigSourceType::CommandLine:
      return "command-line";
    case ConfigSourceType::UserConfig:
      return userConfigPath_.c_str();
    case ConfigSourceType::SystemConfig:
      return systemConfigPath_.c_str();
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
  switch (cs) {
    case ConfigSourceType::Default:
      return {};
    case ConfigSourceType::SystemConfig:
      return absolutePathToThrift(systemConfigPath_);
    case ConfigSourceType::UserConfig:
      return absolutePathToThrift(userConfigPath_);
    case ConfigSourceType::CommandLine:
      return {};
  }
  return {};
}

EdenConfig::EdenConfig(
    ConfigVariables substitutions,
    AbsolutePath userHomePath,
    AbsolutePath userConfigPath,
    AbsolutePath systemConfigDir,
    AbsolutePath systemConfigPath)
    : substitutions_{std::make_shared<ConfigVariables>(
          std::move(substitutions))},
      userConfigPath_{std::move(userConfigPath)},
      systemConfigPath_{std::move(systemConfigPath)} {
  // Force set defaults that require passed arguments
  edenDir.setValue(
      userHomePath + kDefaultEdenDirectory, ConfigSourceType::Default, true);
  userIgnoreFile.setValue(
      userHomePath + kDefaultUserIgnoreFile, ConfigSourceType::Default, true);
  systemIgnoreFile.setValue(
      systemConfigDir + kDefaultSystemIgnoreFile,
      ConfigSourceType::Default,
      true);
}

EdenConfig::EdenConfig(const EdenConfig& source) {
  doCopy(source);
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

EdenConfig& EdenConfig::operator=(const EdenConfig& source) {
  doCopy(source);
  return *this;
}

void EdenConfig::doCopy(const EdenConfig& source) {
  substitutions_ = source.substitutions_;
  userConfigPath_ = source.userConfigPath_;
  systemConfigPath_ = source.systemConfigPath_;

  systemConfigFileStat_ = source.systemConfigFileStat_;
  userConfigFileStat_ = source.userConfigFileStat_;

  // Copy each ConfigSettings from source.
  for (const auto& sectionEntry : source.configMap_) {
    auto& section = sectionEntry.first;
    auto& keyMap = sectionEntry.second;
    for (const auto& kvEntry : keyMap) {
      // Here we are using the assignment operator to copy the configSetting.
      // We are using the base pointer to do the assignment.
      configMap_[section][kvEntry.first]->copyFrom(*kvEntry.second);
    }
  }
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

namespace {
FileChangeReason hasConfigFileChanged(
    AbsolutePath configFileName,
    const std::optional<FileStat>& oldStat) {
  // We are using stat to check for file deltas. Since we don't open file,
  // there is no chance of TOCTOU attack.
  std::optional<FileStat> currentStat;
  auto result = getFileStat(configFileName.c_str());

  // Treat config file as if not present on error.
  // Log error if not ENOENT as they are unexpected and useful for debugging.
  if (result.hasError()) {
    if (result.error() != ENOENT) {
      XLOG(WARN) << "error accessing config file " << configFileName << ": "
                 << folly::errnoStr(result.error());
    }
  } else {
    currentStat = result.value();
  }

  if (oldStat && currentStat) {
    return hasFileChanged(*oldStat, *currentStat);
  } else if (oldStat) {
    return FileChangeReason::SIZE;
  } else if (currentStat) {
    return FileChangeReason::SIZE;
  } else {
    return FileChangeReason::NONE;
  }
}
} // namespace

FileChangeReason EdenConfig::hasUserConfigFileChanged() const {
  return hasConfigFileChanged(getUserConfigPath(), userConfigFileStat_);
}

FileChangeReason EdenConfig::hasSystemConfigFileChanged() const {
  return hasConfigFileChanged(getSystemConfigPath(), systemConfigFileStat_);
}

const AbsolutePath& EdenConfig::getUserConfigPath() const {
  return userConfigPath_;
}

const AbsolutePath& EdenConfig::getSystemConfigPath() const {
  return systemConfigPath_;
}

void EdenConfig::clearAll(ConfigSourceType configSource) {
  for (const auto& sectionEntry : configMap_) {
    for (auto& keyEntry : sectionEntry.second) {
      keyEntry.second->clearValue(configSource);
    }
  }
}

void EdenConfig::loadSystemConfig() {
  clearAll(ConfigSourceType::SystemConfig);
  loadConfig(
      systemConfigPath_, ConfigSourceType::SystemConfig, systemConfigFileStat_);
}

void EdenConfig::loadUserConfig() {
  clearAll(ConfigSourceType::UserConfig);
  loadConfig(
      userConfigPath_, ConfigSourceType::UserConfig, userConfigFileStat_);
}

void EdenConfig::loadConfig(
    AbsolutePathPiece path,
    ConfigSourceType configSource,
    std::optional<FileStat>& configFileStat) {
  // Load the config path and update its stat information
  auto configFd = open(path.copy().c_str(), O_RDONLY);
  if (configFd < 0) {
    if (errno != ENOENT) {
      XLOG(WARN) << "error accessing config file " << path << ": "
                 << folly::errnoStr(errno);
    }
    // TODO: If a config is deleted from underneath, should we clear configs
    // from that file?
    configFileStat = std::nullopt;
    return;
  }
  folly::File configFile(configFd, /*ownsFd=*/true);
  getConfigStat(path, configFile.fd(), configFileStat);
  if (configFd >= 0) {
    parseAndApplyConfigFile(configFd, path, configSource);
  }
}

namespace {
// This is a bit gross.  We have enough type information in the toml
// file to know when an option is a boolean or array, but at the moment our
// intermediate layer stringly-types all the data.  When the upper
// layers want to consume a bool or array, they expect to do so by consuming
// the string representation of it.
// This helper performs the reverse transformation so that we allow
// users to specify their configuration as a true boolean or array type.
cpptoml::option<std::string> itemAsString(
    const std::shared_ptr<cpptoml::table>& currSection,
    const std::string& entryKey) {
  auto valueStr = currSection->get_as<std::string>(entryKey);
  if (valueStr) {
    return valueStr;
  }

  auto valueBool = currSection->get_as<bool>(entryKey);
  if (valueBool) {
    return cpptoml::option<std::string>(*valueBool ? "true" : "false");
  }

  auto valueArray = currSection->get_array(entryKey);
  if (valueArray) {
    // re-serialize using cpp-toml
    std::ostringstream stringifiedValue{};
    stringifiedValue << *valueArray;
    return stringifiedValue.str();
  }

  return {};
}
} // namespace

void EdenConfig::parseAndApplyConfigFile(
    int configFd,
    AbsolutePathPiece configPath,
    ConfigSourceType configSource) {
  std::shared_ptr<cpptoml::table> configRoot;

  try {
    std::string fileContents;
    if (!folly::readFile(configFd, fileContents)) {
      XLOG(WARNING) << "Failed to read config file : " << configPath;
      return;
    }
    std::istringstream is{fileContents};
    cpptoml::parser p{is};
    configRoot = p.parse();
  } catch (const cpptoml::parse_exception& ex) {
    XLOG(WARNING) << "Failed to parse config file : " << configPath
                  << " skipping, error : " << ex.what();
    return;
  }

  // Report unknown sections
  for (const auto& sectionEntry : *configRoot) {
    const auto& sectionName = sectionEntry.first;
    auto configMapEntry = configMap_.find(sectionName);
    if (configMapEntry == configMap_.end()) {
      XLOG(WARNING) << "Ignoring unknown section in eden config: " << configPath
                    << ", key: " << sectionName;
      continue;
    }
    // Load section
    auto currSection = sectionEntry.second->as_table();
    if (currSection) {
      // Report unknown config settings.
      for (const auto& entry : *currSection) {
        const auto& entryKey = entry.first;
        auto configMapKeyEntry = configMapEntry->second.find(entryKey);
        if (configMapKeyEntry == configMapEntry->second.end()) {
          XLOG(WARNING) << "Ignoring unknown key in eden config: " << configPath
                        << ", " << sectionName << ":" << entryKey;
          continue;
        }
        auto valueStr = itemAsString(currSection, entryKey);
        if (valueStr) {
          auto rslt = configMapKeyEntry->second->setStringValue(
              *valueStr, *substitutions_, configSource);
          if (rslt.hasError()) {
            XLOG(WARNING) << "Ignoring invalid config entry " << configPath
                          << " " << sectionName << ":" << entryKey
                          << ", value '" << *valueStr << "' " << rslt.error();
          }
        } else {
          XLOG(WARNING) << "Ignoring invalid config entry " << configPath << " "
                        << sectionName << ":" << entryKey
                        << ", is not a string or boolean";
        }
      }
    }
  }
}

} // namespace facebook::eden
