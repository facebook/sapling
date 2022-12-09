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
#include <folly/Range.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>

#include "eden/fs/eden-config.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/EnumValue.h"

using folly::StringPiece;

namespace facebook::eden {

namespace {

constexpr PathComponentPiece kDefaultUserIgnoreFile{".edenignore"};
constexpr PathComponentPiece kDefaultSystemIgnoreFile{"ignore"};
constexpr PathComponentPiece kDefaultEdenDirectory{".eden"};

void getConfigStat(
    AbsolutePathPiece configPath,
    int configFd,
    struct stat& configStat) {
  int statRslt{-1};
  if (configFd >= 0) {
    statRslt = fstat(configFd, &configStat);
    // Report failure that is not due to ENOENT
    if (statRslt != 0) {
      XLOG(WARN) << "error accessing config file " << configPath << ": "
                 << folly::errnoStr(errno);
    }
  }

  // We use all 0's to check if a file is created/deleted
  if (statRslt != 0) {
    memset(&configStat, 0, sizeof(configStat));
  }
}

std::pair<StringPiece, StringPiece> parseKey(StringPiece fullKey) {
  auto pos = fullKey.find(":");
  if (pos == std::string::npos) {
    EDEN_BUG() << "ConfigSetting key must contain a colon: " << fullKey;
  }

  StringPiece section{fullKey.data(), pos};
  StringPiece key{fullKey.data() + pos + 1, fullKey.end()};

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
  return std::make_unique<EdenConfig>(
      /* userName=*/"testuser",
      /* userID=*/uid_t{},
      /* userHomePath=*/canonicalPath("/tmp"),
      /* userConfigPath=*/canonicalPath("/tmp"),
      /* systemConfigDir=*/canonicalPath("/tmp"),
      /* setSystemConfigPath=*/canonicalPath("/tmp"));
}

std::string EdenConfig::toString(ConfigSource cs) const {
  switch (cs) {
    case ConfigSource::Default:
      return "default";
    case ConfigSource::CommandLine:
      return "command-line";
    case ConfigSource::UserConfig:
      return userConfigPath_.c_str();
    case ConfigSource::SystemConfig:
      return systemConfigPath_.c_str();
  }
  throw std::invalid_argument(
      folly::to<std::string>("invalid config source value: ", enumValue(cs)));
}

std::string EdenConfig::toString() const {
  std::string rslt = fmt::format(
      "[ EdenConfig settings ]\n"
      "userConfigPath={}\n"
      "systemConfigDir={}\n"
      "systemConfigPath={}\n",
      userConfigPath_,
      systemConfigDir_,
      systemConfigPath_);

  rslt += "[ EdenConfig values ]\n";
  for (const auto& sectionEntry : configMap_) {
    auto sectionKey = sectionEntry.first;
    for (const auto& keyEntry : sectionEntry.second) {
      rslt += folly::to<std::string>(
          sectionKey,
          ":",
          keyEntry.first,
          "=",
          keyEntry.second->getStringValue(),
          "\n");
    }
  }
  rslt += "[ EdenConfig sources ]\n";
  for (const auto& sectionEntry : configMap_) {
    auto sectionKey = sectionEntry.first;
    for (const auto& keyEntry : sectionEntry.second) {
      rslt += folly::to<std::string>(
          sectionKey,
          ":",
          keyEntry.first,
          "=",
          EdenConfig::toString(keyEntry.second->getSource()),
          "\n");
    }
  }
  rslt += "]\n";
  return rslt;
}

EdenConfigData EdenConfig::toThriftConfigData() const {
  EdenConfigData result;
  for (const auto& sectionEntry : configMap_) {
    const auto& sectionKey = sectionEntry.first;
    for (const auto& keyEntry : sectionEntry.second) {
      auto keyName = folly::to<std::string>(sectionKey, ":", keyEntry.first);
      auto& configValue = result.values_ref()[keyName];
      *configValue.parsedValue_ref() = keyEntry.second->getStringValue();
      *configValue.source_ref() = keyEntry.second->getSource();
    }
  }
  return result;
}

EdenConfig::EdenConfig(
    std::string userName,
    uid_t userID,
    AbsolutePath userHomePath,
    AbsolutePath userConfigPath,
    AbsolutePath systemConfigDir,
    AbsolutePath systemConfigPath)
    : userName_{std::move(userName)},
      userID_{userID},
      userHomePath_{std::move(userHomePath)},
      userConfigPath_{std::move(userConfigPath)},
      systemConfigPath_{std::move(systemConfigPath)},
      systemConfigDir_{std::move(systemConfigDir)} {
  // Force set defaults that require passed arguments
  edenDir.setValue(
      userHomePath_ + kDefaultEdenDirectory, ConfigSource::Default, true);
  userIgnoreFile.setValue(
      userHomePath_ + kDefaultUserIgnoreFile, ConfigSource::Default, true);
  systemIgnoreFile.setValue(
      systemConfigDir_ + kDefaultSystemIgnoreFile, ConfigSource::Default, true);

  // I have observed Clang on macOS (Xcode 11.6.0) not zero-initialize
  // padding in these members, even though they should be
  // zero-initialized. Explicitly zero.  (Technically, none of this
  // code relies on the padding bits of these stat() results being
  // zeroed, but since we assert it elsewhere to catch bugs,
  // explicitly zero here to be consistent. Another option would be to
  // use std::optional.)
  memset(&systemConfigFileStat_, 0, sizeof(systemConfigFileStat_));
  memset(&userConfigFileStat_, 0, sizeof(userConfigFileStat_));
}

EdenConfig::EdenConfig(const EdenConfig& source) {
  doCopy(source);
}

AbsolutePathPiece EdenConfig::getUserHomePath() const {
  return userHomePath_;
}

const std::string& EdenConfig::getUserName() const {
  return userName_;
}

uid_t EdenConfig::getUserID() const {
  return userID_;
}

std::optional<std::string> EdenConfig::getValueByFullKey(
    folly::StringPiece configKey) const {
  // Throws if the config key is ill-formed.
  auto [sectionKey, entryKey] = parseKey(configKey);

  if (auto* entry =
          folly::get_ptr(configMap_, sectionKey.str(), entryKey.str())) {
    return (*entry)->getStringValue();
  }

  return std::nullopt;
}

EdenConfig& EdenConfig::operator=(const EdenConfig& source) {
  doCopy(source);
  return *this;
}

void EdenConfig::doCopy(const EdenConfig& source) {
  userName_ = source.userName_;
  userID_ = source.userID_;
  userHomePath_ = source.userHomePath_;
  userConfigPath_ = source.userConfigPath_;
  systemConfigPath_ = source.systemConfigPath_;
  systemConfigDir_ = source.systemConfigDir_;

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
  StringPiece fullKeyStr = configSetting->getConfigKey();
  auto [section, key] = parseKey(fullKeyStr);

  auto& keyMap = configMap_[section.str()];
  keyMap[key.str()] = configSetting;
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
    const struct stat& oldStat) {
  struct stat currentStat;

  // We are using stat to check for file deltas. Since we don't open file,
  // there is no chance of TOCTOU attack.
  int rslt = stat(configFileName.c_str(), &currentStat);

  // Treat config file as if not present on error.
  // Log error if not ENOENT as they are unexpected and useful for debugging.
  if (rslt != 0) {
    if (errno != ENOENT) {
      XLOG(WARN) << "error accessing config file " << configFileName << ": "
                 << folly::errnoStr(errno);
    }
    // We use all 0's to check if a file is created/deleted
    memset(&currentStat, 0, sizeof(currentStat));
  }

  return hasFileChanged(currentStat, oldStat);
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

void EdenConfig::clearAll(ConfigSource configSource) {
  for (const auto& sectionEntry : configMap_) {
    for (auto& keyEntry : sectionEntry.second) {
      keyEntry.second->clearValue(configSource);
    }
  }
}

void EdenConfig::loadSystemConfig() {
  clearAll(ConfigSource::SystemConfig);
  loadConfig(
      systemConfigPath_, ConfigSource::SystemConfig, &systemConfigFileStat_);
}

void EdenConfig::loadUserConfig() {
  clearAll(ConfigSource::UserConfig);
  loadConfig(userConfigPath_, ConfigSource::UserConfig, &userConfigFileStat_);
}

void EdenConfig::loadConfig(
    AbsolutePathPiece path,
    ConfigSource configSource,
    struct stat* configFileStat) {
  struct stat configStat;
  // Load the config path and update its stat information
  auto configFd = open(path.copy().c_str(), O_RDONLY);
  if (configFd < 0) {
    if (errno != ENOENT) {
      XLOG(WARN) << "error accessing config file " << path << ": "
                 << folly::errnoStr(errno);
    }
  }
  getConfigStat(path, configFd, configStat);
  memcpy(configFileStat, &configStat, sizeof(struct stat));
  if (configFd >= 0) {
    parseAndApplyConfigFile(configFd, path, configSource);
  }
  SCOPE_EXIT {
    if (configFd >= 0) {
      close(configFd);
    }
  };
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
    ConfigSource configSource) {
  std::shared_ptr<cpptoml::table> configRoot;
  std::map<std::string, std::string> attrMap;
  attrMap["HOME"] = userHomePath_.value();
  attrMap["USER"] = userName_;
  attrMap["USER_ID"] = std::to_string(userID_);
  if (auto certPath = std::getenv("THRIFT_TLS_CL_CERT_PATH")) {
    attrMap["THRIFT_TLS_CL_CERT_PATH"] = certPath;
  }

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
              *valueStr, attrMap, configSource);
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
