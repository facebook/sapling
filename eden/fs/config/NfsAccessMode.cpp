/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/NfsAccessMode.h"

namespace facebook::eden {

namespace {

constexpr auto nfsAccessModeStr = [] {
  std::array<folly::StringPiece, 3> mapping{};
  mapping[folly::to_underlying(NfsAccessMode::Off)] = "off";
  mapping[folly::to_underlying(NfsAccessMode::Log)] = "log";
  mapping[folly::to_underlying(NfsAccessMode::Block)] = "block";
  return mapping;
}();

} // namespace

folly::Expected<NfsAccessMode, std::string>
FieldConverter<NfsAccessMode>::fromString(
    folly::StringPiece value,
    const std::map<std::string, std::string>& /*unused*/) const {
  for (auto i = 0ul; i < nfsAccessModeStr.size(); i++) {
    if (value.equals(nfsAccessModeStr[i], folly::AsciiCaseInsensitive())) {
      return static_cast<NfsAccessMode>(i);
    }
  }

  return folly::makeUnexpected(
      fmt::format("Failed to convert value '{}' to an NfsAccessMode.", value));
}

std::string FieldConverter<NfsAccessMode>::toDebugString(
    NfsAccessMode value) const {
  return nfsAccessModeStr[folly::to_underlying(value)].str();
}

} // namespace facebook::eden
