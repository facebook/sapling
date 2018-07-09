/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenConfig.h"
#include <cpptoml.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/json.h>
#include <folly/logging/xlog.h>
#include <array>

using folly::ByteRange;
using folly::IOBuf;
using folly::Optional;
using folly::StringPiece;
using std::string;
using namespace folly::string_piece_literals;

constexpr folly::StringPiece kHomeEnvVar{"$HOME"};

const facebook::eden::RelativePathPiece kDefaultEdenDirectory{".eden"};
const facebook::eden::RelativePathPiece kDefaultIgnoreFile{"ignore"};
const facebook::eden::AbsolutePath kUnspecifiedDefault{"/"};

namespace {

/**
 * Check if string represents a well-formed file path.
 */
bool isValidAbsolutePath(folly::StringPiece path) {
  // Should we be more strict? (regex based?)
  if (!path.empty() && path.front() == '/') {
    return true;
  }
  return false;
}

template <typename String>
void toAppend(facebook::eden::EdenConfig& ec, String* result) {
  folly::toAppend(ec.toString(), result);
}

} // namespace

namespace facebook {
namespace eden {

std::string EdenConfig::toString(facebook::eden::ConfigSource cs) const {
  switch (cs) {
    case facebook::eden::DEFAULT:
      return "default";
    case facebook::eden::COMMAND_LINE:
      return "command-line";
    case facebook::eden::USER_CONFIG_FILE:
      return userConfigPath_.c_str();
    case facebook::eden::SYSTEM_CONFIG_FILE:
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

EdenConfig::EdenConfig(
    AbsolutePath userHomePath,
    AbsolutePath userConfigPath,
    AbsolutePath systemConfigDir,
    AbsolutePath systemConfigPath)
    : userHomePath_(userHomePath),
      userConfigPath_(userConfigPath),
      systemConfigPath_(systemConfigPath),
      systemConfigDir_(systemConfigDir) {
  // Force set defaults that require passed arguments
  edenDir_.setValue(
      userHomePath_ + kDefaultEdenDirectory, facebook::eden::DEFAULT, true);
  ignoreFile_.setValue(
      userHomePath + kDefaultIgnoreFile, facebook::eden::DEFAULT, true);
  systemIgnoreFile_.setValue(
      systemConfigDir_ + kDefaultIgnoreFile, facebook::eden::DEFAULT, true);
}

EdenConfig::EdenConfig(const EdenConfig& source) {
  doCopy(source);
}

EdenConfig& EdenConfig::operator=(const EdenConfig& source) {
  doCopy(source);
  return *this;
}

void EdenConfig::doCopy(const EdenConfig& source) {
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

const AbsolutePath& EdenConfig::getIgnoreFile() const {
  return ignoreFile_.getValue();
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

void EdenConfig::setIgnoreFile(
    AbsolutePath ignoreFile,
    ConfigSource configSource) {
  return ignoreFile_.setValue(ignoreFile, configSource);
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
  if (currentStat.st_dev != oldStat->st_dev ||
      currentStat.st_ino != oldStat->st_ino ||
      currentStat.st_mtime != oldStat->st_mtime) {
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
  clearAll(facebook::eden::SYSTEM_CONFIG_FILE);
  loadConfig(
      systemConfigPath_,
      facebook::eden::SYSTEM_CONFIG_FILE,
      &systemConfigFileStat_);
}

void EdenConfig::loadUserConfig() {
  clearAll(facebook::eden::USER_CONFIG_FILE);
  loadConfig(
      userConfigPath_, facebook::eden::USER_CONFIG_FILE, &userConfigFileStat_);
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

void EdenConfig::parseAndApplyConfigFile(
    int configFd,
    AbsolutePathPiece configPath,
    ConfigSource configSource) {
  std::shared_ptr<cpptoml::table> configRoot;
  std::map<std::string, std::string> attrMap;
  attrMap["HOME"] = userHomePath_.value();

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
        auto valueStr = currSection->get_as<std::string>(entryKey);
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
                        << ", is not a string";
        }
      }
    }
  }
}

folly::Expected<AbsolutePath, std::string> FieldConverter<AbsolutePath>::
operator()(
    folly::StringPiece value,
    const std::map<std::string, std::string>& convData) const {
  auto sString = value.str();
  auto it = convData.find("HOME");
  if (it != convData.end()) {
    auto idx = value.find(kHomeEnvVar);
    if (idx != std::string::npos) {
      sString.replace(idx, kHomeEnvVar.size(), it->second);
    }
  }
  if (!::isValidAbsolutePath(sString)) {
    return folly::makeUnexpected<std::string>(folly::to<std::string>(
        "Cannot convert value '", value, "' to an absolute path"));
  }
  // normalizeBestEffort typically will not throw, but, we want to handle
  // cases where it does, eg. getcwd fails.
  try {
    return facebook::eden::normalizeBestEffort(sString);
  } catch (const std::exception& ex) {
    return folly::makeUnexpected<string>(folly::to<std::string>(
        "Failed to convert value '",
        value,
        "' to an absolute path, error : ",
        ex.what()));
  }
}

} // namespace eden
} // namespace facebook
