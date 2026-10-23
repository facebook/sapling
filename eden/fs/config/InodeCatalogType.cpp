/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/InodeCatalogType.h"

#include <algorithm>
#include <array>
#include <utility>

namespace facebook::eden {

namespace {

constexpr std::array<std::pair<InodeCatalogType, std::string_view>, 4>
    kInodeCatalogTypeNames{{
        {InodeCatalogType::Legacy, "Legacy"},
        {InodeCatalogType::Sqlite, "Sqlite"},
        {InodeCatalogType::InMemory, "InMemory"},
        {InodeCatalogType::LegacyEphemeral, "LegacyEphemeral"},
    }};

} // namespace

folly::Expected<InodeCatalogType, std::string> inodeCatalogTypeFromString(
    std::string_view value) {
  for (const auto& [type, name] : kInodeCatalogTypeNames) {
    if (std::equal(
            value.begin(),
            value.end(),
            name.begin(),
            name.end(),
            folly::AsciiCaseInsensitive())) {
      return type;
    }
  }

  return folly::makeUnexpected(
      fmt::format(
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
  for (const auto& [type, name] : kInodeCatalogTypeNames) {
    if (type == value) {
      return std::string{name};
    }
  }
  return fmt::format("InodeCatalogType({})", folly::to_underlying(value));
}

} // namespace facebook::eden
