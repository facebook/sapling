/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/RestrictedContentMode.h"

#include "eden/fs/config/FieldConverter.h"

namespace facebook::eden {

namespace {

constexpr auto restrictedContentModeStr = [] {
  std::array<folly::StringPiece, 2> mapping{};
  mapping[folly::to_underlying(RestrictedContentMode::Restricted)] =
      "restricted";
  mapping[folly::to_underlying(RestrictedContentMode::Omitted)] = "omitted";
  return mapping;
}();

} // namespace

folly::Expected<RestrictedContentMode, std::string>
FieldConverter<RestrictedContentMode>::fromString(
    folly::StringPiece value,
    const std::map<std::string, std::string>& /*unused*/) const {
  for (auto i = 0ul; i < restrictedContentModeStr.size(); i++) {
    if (value.equals(
            restrictedContentModeStr[i], folly::AsciiCaseInsensitive())) {
      return static_cast<RestrictedContentMode>(i);
    }
  }

  return folly::makeUnexpected(
      fmt::format(
          "Failed to convert value '{}' to a RestrictedContentMode.", value));
}

std::string FieldConverter<RestrictedContentMode>::toDebugString(
    RestrictedContentMode value) const {
  return restrictedContentModeStr[folly::to_underlying(value)].str();
}

} // namespace facebook::eden
