/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Range.h>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

namespace facebook {
namespace eden {

constexpr folly::StringPiece kVmRSSKey{"VmRSS"};
constexpr folly::StringPiece kKBytes{"kB"};
constexpr folly::StringPiece kLinuxProcStatusPath{"/proc/self/status"};
constexpr folly::StringPiece kLinuxProcSmapsPath{"/proc/self/smaps"};

namespace proc_util {

/**
 * Trim leading and trailing delimiter characters from passed string.
 * @return the modified string.
 */
std::string& trim(std::string& str, const std::string& delim = " \t\n\v\f\r");

/**
 * Extract the key value pair form the passed line.  The delimiter
 * separates the key and value. Whitespace is trimmed from the result strings.
 * @return key/value pair or two empty strings if number of segments != 2.
 */
std::pair<std::string, std::string> getKeyValuePair(
    const std::string& line,
    const std::string& delim);

/**
 * Parse the passed stream (typically /proc/self/smaps).
 * Callers should handle io exceptions (catch std::ios_base::failure).
 * @return a list of maps for each entry.
 */
std::vector<std::unordered_map<std::string, std::string>> parseProcSmaps(
    std::istream& input);

/**
 * Load the contents of the linux proc/smaps from kLinuxProcSmapsPath.
 * It handles file operations and exceptions.  It makes use of
 * parseProcSmaps for parsing file contents.
 * @return a vector of maps with file contents or empty vector on error.
 */
std::vector<std::unordered_map<std::string, std::string>> loadProcSmaps();

/**
 * Load the contents of the linux proc/smaps file from procSmapsPath.
 * It handles file operations and exceptions.  It makes use of
 * parseProcSmaps for parsing file contents.
 * It is provided to test loadProcSmaps.
 * @return a vector of maps with file contents or empty vector on error.
 */
std::vector<std::unordered_map<std::string, std::string>> loadProcSmaps(
    folly::StringPiece procSmapsPath);

/**
 * Calculate the private bytes used by the eden process. The calculation
 * is done by loading, parsing and summing values in /proc/self/smaps file.
 * @return memory usage in bytes or 0 if the value could not be determined.
 * On non-linux platforms, 0 will be returned.
 */
std::optional<uint64_t> calculatePrivateBytes();

/**
 * Calculate the private byte count based on passed vector of maps.
 * Intended for use by calculatePrivateBytes().
 * @see parseProcSmaps to create the map.
 */
std::optional<uint64_t> calculatePrivateBytes(
    std::vector<std::unordered_map<std::string, std::string>> smapsListOfMaps);

/**
 * Parse the passed stream (typically /proc/self/status).
 * Callers should handle io exceptions (catch std::ios_base::failure).
 * @return a map of key value pairs from the file.
 */
std::unordered_map<std::string, std::string> parseProcStatus(
    std::istream& input);

/**
 * Load the contents of the linux system file kLinuxProcStatusPath.
 * It catches file operations and exceptions.  It makes use of
 * parseProcStatus for parsing file contents.
 * @return a map of file contents or an empty map on error.
 */
std::unordered_map<std::string, std::string> loadProcStatus();

/**
 * Load the contents of the linux proc/status file from procStatusPath.
 * It catches file operations and exceptions.  It makes use of
 * parseProcStatus for parsing file contents.
 * Intended to test the loadProcStatus().
 * @return a map of file contents or an empty map on error.
 */
std::unordered_map<std::string, std::string> loadProcStatus(
    folly::StringPiece procStatusPath);

/**
 *  Retrieve the identified value based on the passed key.
 *  The value must present, a valid unsigned long and contain the
 *  trailing unitSuffix. Example use:
 *  getUnsignedLongLongValue(procMap, "VmRSS", "kB").
 *  If the value does not exist or is invalid, 0 will be returned.
 *  @see loadProcStatMap for parsing the /proc/self/status file.
 */
std::optional<uint64_t> getUnsignedLongLongValue(
    const std::unordered_map<std::string, std::string>& procStatMap,
    const std::string& key,
    const std::string& unitSuffix);
} // namespace proc_util
} // namespace eden
} // namespace facebook
