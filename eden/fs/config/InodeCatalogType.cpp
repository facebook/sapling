/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/InodeCatalogType.h"

namespace facebook::eden {

namespace {

constexpr auto inodeCatalogTypeStr = [] {
  std::array<string_view, 8> mapping{};
  mapping[folly::to_underlying(InodeCatalogType::Legacy)] = "Legacy";
  mapping[folly::to_underlying(InodeCatalogType::Sqlite)] = "Sqlite";
  mapping[folly::to_underlying(InodeCatalogType::InMemory)] = "InMemory";
  mapping[folly::to_underlying(InodeCatalogType::LMDB)] = "LMDB";
  return mapping;
}();

}

folly::Expected<InodeCatalogType, std::string> inodeCatalogTypeFromString(
    std::string_view value) {
  for (auto type = 0ul; type < inodeCatalogTypeStr.size(); type++) {
    auto typeStr = inodeCatalogTypeStr[type];
    if (std::equal(
            value.begin(),
            value.end(),
            typeStr.begin(),
            typeStr.end(),
            folly::AsciiCaseInsensitive())) {
      return static_cast<InodeCatalogType>(type);
    }
  }

  return folly::makeUnexpected(fmt::format(
      "Failed to convert value '{}' to a InodeCatalogType.", value));
}

folly::Expected<InodeCatalogType, std::string>
FieldConverter<InodeCatalogType>::fromString(
    folly::StringPiece value,
    const std::map<std::string, std::string>& /*unused*/) const {
  return inodeCatalogTypeFromString(value);
}

std::string FieldConverter<InodeCatalogType>::toDebugString(
    InodeCatalogType value) const {
  return std::string{inodeCatalogTypeStr[folly::to_underlying(value)]};
}

} // namespace facebook::eden
