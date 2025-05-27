/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ProcUtil.h"

#include <array>
#include <fstream>
#include <optional>
#include <vector>

#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>
#include "eden/common/utils/FileUtils.h"

#ifdef __APPLE__
#include <libproc.h> // @manual
#include <mach/mach_init.h> // @manual
#include <mach/task.h> // @manual
#include <mach/task_info.h> // @manual
#include <sys/proc_info.h> // @manual
#endif

#ifdef _WIN32
#include <psapi.h> // @manual
#endif

using folly::StringPiece;
using std::optional;

namespace facebook::eden::proc_util {

namespace {
#ifdef __APPLE__
optional<MemoryStats> readMemoryStatsApple() {
  mach_task_basic_info_data_t taskinfo{};
  mach_msg_type_number_t outCount = MACH_TASK_BASIC_INFO_COUNT;
  auto result = task_info(
      mach_task_self(),
      MACH_TASK_BASIC_INFO,
      reinterpret_cast<task_info_t>(&taskinfo),
      &outCount);
  if (result != KERN_SUCCESS) {
    return std::nullopt;
  }

  MemoryStats ms;
  ms.vsize = taskinfo.virtual_size;
  ms.resident = taskinfo.resident_size;
  return ms;
}
#endif

#ifdef _WIN32
optional<MemoryStats> readMemoryStatsWin() {
  HANDLE proc = GetCurrentProcess();
  PROCESS_MEMORY_COUNTERS memoryCounters{};

  if (!GetProcessMemoryInfo(proc, &memoryCounters, sizeof(memoryCounters))) {
    return std::nullopt;
  }

  MEMORYSTATUSEX memStatus{};
  memStatus.dwLength = sizeof(memStatus);
  if (!GlobalMemoryStatusEx(&memStatus)) {
    return std::nullopt;
  }

  MemoryStats ms;
  ms.vsize = memStatus.ullTotalVirtual - memStatus.ullAvailVirtual;
  ms.resident = memoryCounters.WorkingSetSize;

  return ms;
}
#endif
} // namespace

optional<MemoryStats> readMemoryStats() {
#ifdef __APPLE__
  return readMemoryStatsApple();
#elif _WIN32
  return readMemoryStatsWin();
#else
  return readStatmFile(canonicalPath("/proc/self/statm"));
#endif
}

#ifndef _WIN32
optional<MemoryStats> readStatmFile(AbsolutePathPiece filename) {
  auto contents = readFile(filename);
  if (!contents.hasValue()) {
    return std::nullopt;
  }
  auto pageSize = sysconf(_SC_PAGESIZE);
  if (pageSize == -1) {
    return std::nullopt;
  }
  return parseStatmFile(contents.value(), pageSize);
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
  stats.vsize = pageSize * values[0];
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
    if (line.find('-') != std::string::npos) {
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

std::optional<size_t> calculatePrivateBytes(
    std::vector<std::unordered_map<std::string, std::string>> smapsListOfMaps) {
  size_t count{0};
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
#endif

std::optional<size_t> calculatePrivateBytes() {
#ifndef _WIN32
  try {
    std::ifstream input(kLinuxProcSmapsPath.data());
    return calculatePrivateBytes(parseProcSmaps(input));
  } catch (const std::exception& ex) {
    XLOG(WARN) << "Failed to parse file " << kLinuxProcSmapsPath << ex.what();
    return std::nullopt;
  }
#else
  return std::nullopt;
#endif
}

ProcessList readProcessIdsForPath([[maybe_unused]] const AbsolutePath& path) {
  ProcessList pids;
#ifdef __APPLE__
  // Obtain the number of bytes to allocate for the pids buffer.
  int32_t pids_size = proc_listpidspath(
      PROC_ALL_PIDS,
      0,
      path.c_str(),
      PROC_LISTPIDSPATH_PATH_IS_VOLUME,
      nullptr,
      0);

  // There is a race-condition here, where processes could be started after the
  // call above to compute the amount of storage required. The maximum number of
  // processes for a given machine is based on both OS version and hardware
  // constraints so there is not a known compile time value nor is there a
  // known OS call to obtain it. However, calling proc_listpidspath with less
  // storage than required does not result in a buffer overflow or an error -
  // rather it truncates the results to fit within the buffer provided. This is
  // acceptable in most use cases as this is expected to be a rare occurrence.
  if (pids_size > 0) {
    // Resize buffer and fetch pids.
    pids.resize(pids_size / sizeof(pid_t));
    pids_size = proc_listpidspath(
        PROC_ALL_PIDS,
        0,
        path.c_str(),
        PROC_LISTPIDSPATH_PATH_IS_VOLUME,
        pids.data(),
        pids_size);
  }

  // If anything went wrong, resize buffer to 0 size.
  if (pids_size < 0) {
    XLOGF(
        INFO,
        "proc_listpidspath failed: {} ()",
        folly::errnoStr(pids_size),
        pids_size);
    pids.resize(0);
  } else {
    // Resize buffer to actual size.
    pids.resize(pids_size / sizeof(pid_t));
  }
#endif

  return pids;
}

} // namespace facebook::eden::proc_util
