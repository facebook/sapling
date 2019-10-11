/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ProcUtil.h"

#include <array>
#include <fstream>
#include <vector>

#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>

using folly::StringPiece;
using std::optional;

namespace facebook {
namespace eden {
namespace proc_util {

optional<MemoryStats> readMemoryStats() {
  return readStatmFile("/proc/self/statm");
}

optional<MemoryStats> readStatmFile(const char* filename) {
  std::string contents;
  if (!folly::readFile(filename, contents)) {
    return std::nullopt;
  }
  auto pageSize = sysconf(_SC_PAGESIZE);
  if (pageSize == -1) {
    return std::nullopt;
  }
  return parseStatmFile(contents, pageSize);
}

optional<MemoryStats> parseStatmFile(StringPiece data, size_t pageSize) {
  std::array<size_t, 7> values;
  for (size_t& value : values) {
    auto parseResult = folly::parseTo(data, value);
    if (parseResult.hasError()) {
      return std::nullopt;
    }
    data = parseResult.value();
  }

  MemoryStats stats{};
  stats.size = pageSize * values[0];
  stats.resident = pageSize * values[1];
  stats.shared = pageSize * values[2];
  stats.text = pageSize * values[3];
  // values[4] is always 0 since Linux 2.6
  stats.data = pageSize * values[5];
  // values[6] is always 0 since Linux 2.6

  return stats;
}

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
