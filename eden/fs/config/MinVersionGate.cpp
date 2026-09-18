/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/MinVersionGate.h"

#include <fmt/format.h>
#include <folly/Conv.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>

#include "eden/fs/eden-config.h"

namespace facebook::eden {

namespace {

constexpr std::string_view kMinVersionKeySeparator = "@min-version=";

bool isAsciiDigit(char c) {
  return c >= '0' && c <= '9';
}

size_t leadingDigitCount(std::string_view s) {
  size_t count = 0;
  while (count < s.size() && isAsciiDigit(s[count])) {
    ++count;
  }
  return count;
}

std::optional<uint32_t> parseDigits(std::string_view digits) {
  auto parsed = folly::tryTo<uint32_t>(folly::StringPiece{digits});
  if (!parsed.hasValue()) {
    return std::nullopt;
  }
  return parsed.value();
}

std::optional<EdenVersion> computeBuildEdenVersion() {
  std::string_view version{EDEN_VERSION};
  if (version.empty()) {
    return std::nullopt;
  }
  auto versionString = fmt::format("{}-{}", version, EDEN_RELEASE);
  auto parsed = parseEdenVersion(versionString);
  if (!parsed) {
    XLOGF(
        ERR,
        "Unable to parse EdenFS package version {}; version-gated config entries will not apply",
        versionString);
    return EdenVersion{};
  }
  return parsed;
}

} // namespace

std::optional<EdenVersion> parseEdenVersion(std::string_view version) {
  constexpr size_t kDateDigits = 8;
  if (leadingDigitCount(version) < kDateDigits) {
    return std::nullopt;
  }
  auto date = parseDigits(version.substr(0, kDateDigits));
  if (!date) {
    return std::nullopt;
  }
  EdenVersion result;
  result.date = *date;

  auto rest = version.substr(kDateDigits);
  if (!rest.empty() && rest[0] == '-') {
    rest.remove_prefix(1);
    auto timeDigits = rest.substr(0, leadingDigitCount(rest));
    if (!timeDigits.empty()) {
      auto time = parseDigits(timeDigits);
      if (!time) {
        return std::nullopt;
      }
      result.time = *time;
    }
  }
  return result;
}

std::optional<EdenVersion> getBuildEdenVersion() {
  static const std::optional<EdenVersion> version = computeBuildEdenVersion();
  return version;
}

std::optional<MinVersionGatedKey> parseMinVersionGatedKey(
    std::string_view key) {
  auto pos = key.find(kMinVersionKeySeparator);
  if (pos == std::string_view::npos) {
    return std::nullopt;
  }
  return MinVersionGatedKey{
      key.substr(0, pos), key.substr(pos + kMinVersionKeySeparator.size())};
}

} // namespace facebook::eden
