/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/ReaddirPrefetch.h"

namespace facebook::eden {

namespace {

constexpr auto readdirPrefetchStr = [] {
  std::array<folly::StringPiece, 4> mapping{};
  mapping[folly::to_underlying(ReaddirPrefetch::None)] = "none";
  mapping[folly::to_underlying(ReaddirPrefetch::Files)] = "files";
  mapping[folly::to_underlying(ReaddirPrefetch::Trees)] = "trees";
  mapping[folly::to_underlying(ReaddirPrefetch::Both)] = "both";
  return mapping;
}();

}

folly::Expected<ReaddirPrefetch, std::string>
FieldConverter<ReaddirPrefetch>::fromString(
    folly::StringPiece value,
    const std::map<std::string, std::string>& /*unused*/) const {
  for (auto i = 0ul; i < readdirPrefetchStr.size(); i++) {
    if (value.equals(readdirPrefetchStr[i], folly::AsciiCaseInsensitive())) {
      return static_cast<ReaddirPrefetch>(i);
    }
  }

  return folly::makeUnexpected(
      fmt::format("Failed to convert value '{}' to a ReaddirPrefetch.", value));
}

std::string FieldConverter<ReaddirPrefetch>::toDebugString(
    ReaddirPrefetch value) const {
  return readdirPrefetchStr[folly::to_underlying(value)].str();
}

} // namespace facebook::eden
