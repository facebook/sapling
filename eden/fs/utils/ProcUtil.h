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

struct MemoryStats {
  size_t size; // Total VM Size, in bytes
  size_t resident; // Resident set size, in bytes
  size_t shared; // Resident shared bytes (file mappings + shared memory)
  size_t text; // text (code) bytes
  size_t data; // data + stack bytes
};

/**
 * Read the memory stats for the current process.
 *
 * Returns std::nullopt if an error occurs reading or parsing the data.
 */
std::optional<MemoryStats> readMemoryStats();

/**
 * Read a /proc/<pid>/statm file and return the results as a MemoryStats object.
 *
 * Returns std::nullopt if an error occurs reading or parsing the data.
 */
std::optional<MemoryStats> readStatmFile(const char* filename);

/**
 * Parse the contents of a /proc/<pid>/statm file.
 */
std::optional<MemoryStats> parseStatmFile(
    folly::StringPiece data,
    size_t pageSize);

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

} // namespace proc_util
} // namespace eden
} // namespace facebook
