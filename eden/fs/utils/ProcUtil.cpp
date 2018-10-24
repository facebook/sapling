/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/ProcUtil.h"
#include <folly/logging/xlog.h>
#include <fstream>
#include <vector>

namespace facebook {
namespace eden {
namespace proc_util {

std::string& trim(std::string& str, const std::string& delim) {
  str.erase(0, str.find_first_not_of(delim));
  str.erase(str.find_last_not_of(delim) + 1);
  return str;
}

std::pair<std::string, std::string> getKeyValuePair(
    const std::string& line,
    const std::string& delim) {
  std::vector<std::string> v;
  std::pair<std::string, std::string> result;
  folly::split(delim, line, v);
  if (v.size() == 2) {
    result.first = trim(v.front());
    result.second = trim(v.back());
  }
  return result;
}

std::unordered_map<std::string, std::string> parseProcStatus(
    std::istream& input) {
  std::unordered_map<std::string, std::string> statMap;
  for (std::string line; getline(input, line);) {
    auto keyValue = getKeyValuePair(line, ":");
    if (!keyValue.first.empty()) {
      statMap[keyValue.first] = keyValue.second;
    } else {
      XLOG(WARN) << "Failed to parse /proc/self/status, line : " << line;
    }
  }
  return statMap;
}

std::unordered_map<std::string, std::string> loadProcStatus() {
  return loadProcStatus(kLinuxProcStatusPath);
}

std::unordered_map<std::string, std::string> loadProcStatus(
    folly::StringPiece procStatusPath) {
  try {
    std::ifstream input(procStatusPath.data());
    return parseProcStatus(input);
  } catch (const std::exception& ex) {
    XLOG(WARN) << "Failed to parse proc/status file : " << ex.what();
  }
  return std::unordered_map<std::string, std::string>();
}

std::optional<uint64_t> getUnsignedLongLongValue(
    const std::unordered_map<std::string, std::string>& procStatMap,
    const std::string& key,
    const std::string& unitSuffix) {
  std::optional<uint64_t> value;
  const auto& pos = procStatMap.find(key);
  if (pos != procStatMap.end()) {
    auto valString = pos->second;
    if (valString.find(unitSuffix) != std::string::npos) {
      valString = valString.substr(0, valString.size() - unitSuffix.size());
      try {
        value = std::stoull(valString);
      } catch (const std::invalid_argument& ex) {
        XLOG(WARN) << "Failed to extract long from proc/status value ''"
                   << valString << "' error: " << ex.what();
        return std::nullopt;
      } catch (const std::out_of_range& ex) {
        XLOG(WARN) << "Failed to extract long from proc/status value ''"
                   << valString << "' error: " << ex.what();
        return std::nullopt;
      }
    }
  }
  return value;
} // namespace eden

std::vector<std::unordered_map<std::string, std::string>> parseProcSmaps(
    std::istream& input) {
  std::vector<std::unordered_map<std::string, std::string>> entryList;
  bool headerFound{false};
  std::unordered_map<std::string, std::string> currentMap;

  for (std::string line; getline(input, line);) {
    if (line.find("-") != std::string::npos) {
      if (!currentMap.empty()) {
        entryList.push_back(currentMap);
        currentMap.clear();
      }
      headerFound = true;
    } else {
      if (!headerFound) {
        XLOG(WARN) << "Failed to parse smaps file ";
        continue;
      }
      auto keyValue = getKeyValuePair(line, ":");
      if (!keyValue.first.empty()) {
        currentMap[keyValue.first] = keyValue.second;
      } else {
        XLOG(WARN) << "Failed to parse smaps field in smaps file ";
      }
    }
  }
  if (!currentMap.empty()) {
    entryList.push_back(currentMap);
  }
  return entryList;
}

std::vector<std::unordered_map<std::string, std::string>> loadProcSmaps() {
  return loadProcSmaps(kLinuxProcSmapsPath);
}

std::vector<std::unordered_map<std::string, std::string>> loadProcSmaps(
    folly::StringPiece procSmapsPath) {
  try {
    std::ifstream input(procSmapsPath.data());
    return parseProcSmaps(input);
  } catch (const std::exception& ex) {
    XLOG(WARN) << "Failed to parse memory usage: " << ex.what();
  }
  return std::vector<std::unordered_map<std::string, std::string>>();
}

std::optional<uint64_t> calculatePrivateBytes(
    std::vector<std::unordered_map<std::string, std::string>> smapsListOfMaps) {
  uint64_t count{0};
  for (auto currentMap : smapsListOfMaps) {
    auto iter = currentMap.find("Private_Dirty");
    if (iter != currentMap.end()) {
      auto& entry = iter->second;
      if (entry.rfind(" kB") != std::string::npos) {
        auto countString = entry.substr(0, entry.size() - 3);
        try {
          count += std::stoull(countString) * 1024;
        } catch (const std::invalid_argument& ex) {
          XLOG(WARN) << "Failed to extract long from /proc/smaps value ''"
                     << countString << "' error: " << ex.what();
          return std::nullopt;
        } catch (const std::out_of_range& ex) {
          XLOG(WARN) << "Failed to extract long from proc/status value ''"
                     << countString << "' error: " << ex.what();
          return std::nullopt;
        }
      } else {
        XLOG(WARN) << "Failed to find Private_Dirty units in: "
                   << kLinuxProcSmapsPath;
        return std::nullopt;
      }
    }
  }
  return count;
}

std::optional<uint64_t> calculatePrivateBytes() {
  try {
    std::ifstream input(kLinuxProcSmapsPath.data());
    return calculatePrivateBytes(parseProcSmaps(input));
  } catch (const std::exception& ex) {
    XLOG(WARN) << "Failed to parse file " << kLinuxProcSmapsPath << ex.what();
    return std::nullopt;
  }
}
} // namespace proc_util
} // namespace eden
} // namespace facebook
