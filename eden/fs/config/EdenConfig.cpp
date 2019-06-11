/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/config/EdenConfig.h"

#include <cpptoml.h> // @manual=fbsource//third-party/cpptoml:cpptoml
#include <array>

#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/json.h>
#include <folly/logging/xlog.h>

#include "eden/fs/config/FileChangeMonitor.h"

#ifdef _WIN32
#include "eden/fs/win/utils/Stub.h" // @manual
#endif

using folly::StringPiece;
using std::optional;
using std::string;

const facebook::eden::RelativePathPiece kDefaultEdenDirectory{".eden"};
const facebook::eden::RelativePathPiece kDefaultUserIgnoreFile{".edenignore"};
const facebook::eden::RelativePathPiece kDefaultSystemIgnoreFile{"ignore"};
const facebook::eden::AbsolutePath kUnspecifiedDefault{"/"};

namespace {
template <typename String>
void toAppend(facebook::eden::EdenConfig& ec, String* result) {
  folly::toAppend(ec.toString(), result);
}
} // namespace

namespace facebook {
namespace eden {

std::string EdenConfig::toString(facebook::eden::ConfigSource cs) const {
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
      folly::to<string>("invalid config source value: ", static_cast<int>(cs)));
}

std::string EdenConfig::toString() const {
  std::string rslt;
  rslt += folly::to<std::string>(
      "[ EdenConfig settings ]\n",
      "userConfigPath=",
      userConfigPath_,
      "\n"
      "systemConfigDir=",
      systemConfigDir_,
      "\n"
      "systemConfigPath=",
      systemConfigPath_,
      "\n");

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
      auto keyName = folly::to<string>(sectionKey, ":", keyEntry.first);
      auto& configValue = result.values[keyName];
      configValue.parsedValue = keyEntry.second->getStringValue();
      configValue.source = keyEntry.second->getSource();
    }
  }
  return result;
}

EdenConfig::EdenConfig(
    folly::StringPiece userName,
    uid_t userID,
    AbsolutePath userHomePath,
    AbsolutePath userConfigPath,
    AbsolutePath systemConfigDir,
    AbsolutePath systemConfigPath)
    : userName_(userName),
      userID_(userID),
      userHomePath_(userHomePath),
      userConfigPath_(userConfigPath),
      systemConfigPath_(systemConfigPath),
      systemConfigDir_(systemConfigDir) {
  // Force set defaults that require passed arguments
  edenDir_.setValue(
      userHomePath_ + kDefaultEdenDirectory, ConfigSource::Default, true);
  userIgnoreFile_.setValue(
      userHomePath + kDefaultUserIgnoreFile, ConfigSource::Default, true);
  systemIgnoreFile_.setValue(
      systemConfigDir_ + kDefaultSystemIgnoreFile, ConfigSource::Default, true);
  clientCertificate_.setValue(kUnspecifiedDefault, ConfigSource::Default, true);
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
  auto pos = fullKeyStr.find(":");
  if (pos != std::string::npos) {
    StringPiece section(fullKeyStr.data(), pos);
    StringPiece key(fullKeyStr.data() + pos + 1, fullKeyStr.end());
    auto& keyMap = configMap_[section.str()];
    keyMap[key.str()] = configSetting;
  }
}

const AbsolutePath& EdenConfig::getEdenDir() const {
  return edenDir_.getValue();
}

const AbsolutePath& EdenConfig::getSystemIgnoreFile() const {
  return systemIgnoreFile_.getValue();
}

const AbsolutePath& EdenConfig::getUserIgnoreFile() const {
  return userIgnoreFile_.getValue();
}

const optional<AbsolutePath> EdenConfig::getClientCertificate() const {
  auto value = clientCertificate_.getValue();

  if (value == kUnspecifiedDefault) {
    return std::nullopt;
  }
  return value;
}

bool EdenConfig::getUseMononoke() const {
  return useMononoke_.getValue();
}

const std::string& EdenConfig::getMononokeTierName() const {
  return mononokeTierName_.getValue();
}

std::optional<std::string> EdenConfig::getMononokeHostName() const {
  auto value = mononokeHostName_.getValue();
  if (value.empty()) {
    return std::nullopt;
  }
  return value;
}

uint16_t EdenConfig::getMononokePort() const {
  return mononokePort_.getValue();
}

const std::string& EdenConfig::getMononokeConnectionType() const {
  return mononokeConnectionType_.getValue();
}

void EdenConfig::setUserConfigPath(AbsolutePath userConfigPath) {
  userConfigPath_ = userConfigPath;
}
void EdenConfig::setSystemConfigDir(AbsolutePath systemConfigDir) {
  systemConfigDir_ = systemConfigDir;
}
void EdenConfig::setSystemConfigPath(AbsolutePath systemConfigPath) {
  systemConfigPath_ = systemConfigPath;
}

void EdenConfig::setSystemIgnoreFile(
    AbsolutePath systemIgnoreFile,
    ConfigSource configSource) {
  return systemIgnoreFile_.setValue(systemIgnoreFile, configSource);
}

void EdenConfig::setEdenDir(AbsolutePath edenDir, ConfigSource configSource) {
  return edenDir_.setValue(edenDir, configSource);
}

void EdenConfig::setUserIgnoreFile(
    AbsolutePath userIgnoreFile,
    ConfigSource configSource) {
  return userIgnoreFile_.setValue(userIgnoreFile, configSource);
}

void EdenConfig::setClientCertificate(
    AbsolutePath clientCertificate,
    ConfigSource configSource) {
  return clientCertificate_.setValue(clientCertificate, configSource);
}

void EdenConfig::setUseMononoke(bool useMononoke, ConfigSource configSource) {
  return useMononoke_.setValue(useMononoke, configSource);
}

bool hasConfigFileChanged(
    AbsolutePath configFileName,
    const struct stat* oldStat) {
  bool fileChangeDetected{false};
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
  if (!equalStats(currentStat, *oldStat)) {
    fileChangeDetected = true;
  }
  return fileChangeDetected;
}

bool EdenConfig::hasUserConfigFileChanged() const {
  return hasConfigFileChanged(getUserConfigPath(), &userConfigFileStat_);
}

bool EdenConfig::hasSystemConfigFileChanged() const {
  return hasConfigFileChanged(getSystemConfigPath(), &systemConfigFileStat_);
}

const AbsolutePath& EdenConfig::getUserConfigPath() const {
  return userConfigPath_;
}

const AbsolutePath& EdenConfig::getSystemConfigPath() const {
  return systemConfigPath_;
}

const AbsolutePath& EdenConfig::getSystemConfigDir() const {
  return systemConfigDir_;
}

void EdenConfig::clearAll(ConfigSource configSource) {
  for (const auto& sectionEntry : configMap_) {
    for (auto& keyEntry : sectionEntry.second) {
      keyEntry.second->clearValue(configSource);
    }
  }
}

static void getConfigStat(
    AbsolutePathPiece configPath,
    int configFd,
    struct stat* configStat) {
  int statRslt{-1};
  if (configFd >= 0) {
    statRslt = fstat(configFd, configStat);
    // Report failure that is not due to ENOENT
    if (statRslt != 0) {
      XLOG(WARN) << "error accessing config file " << configPath << ": "
                 << folly::errnoStr(errno);
    }
  }

  // We use all 0's to check if a file is created/deleted
  if (statRslt != 0) {
    memset(configStat, 0, sizeof(struct stat));
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
  getConfigStat(path, configFd, &configStat);
  memcpy(configFileStat, &configStat, sizeof(struct stat));
  if (configFd >= 0) {
    parseAndApplyConfigFile(configFd, path, configSource);
  }
  SCOPE_EXIT {
    close(configFd);
  };
}

namespace {
// This is a bit gross.  We have enough type information in the toml
// file to know when an option is a boolean, but at the moment our
// intermediate layer stringly-types all the data.  When the upper
// layers want to consume a bool, they expect to do so by consuming
// the string representation of it.
// This helper performs the reverse transformation so that we allow
// users to specify their configuration as a true boolean type.
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

} // namespace eden
} // namespace facebook
