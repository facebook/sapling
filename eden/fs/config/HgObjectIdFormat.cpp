/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/HgObjectIdFormat.h"

namespace facebook::eden {

namespace {
constexpr std::pair<HgObjectIdFormat, folly::StringPiece> kMapping[] = {
    {HgObjectIdFormat::WithPath, "withpath"},
    {HgObjectIdFormat::HashOnly, "hashonly"},
};
}

folly::Expected<HgObjectIdFormat, std::string>
FieldConverter<HgObjectIdFormat>::fromString(
    folly::StringPiece value,
    const std::map<std::string, std::string>& /*unused*/) const {
  for (auto [v, name] : kMapping) {
    if (value == name) {
      return v;
    }
  }

  return folly::makeUnexpected(fmt::format(
      "Failed to convert value '{}' to an HgObjectIdFormat", value));
}

std::string FieldConverter<HgObjectIdFormat>::toDebugString(
    HgObjectIdFormat value) const {
  for (auto [v, name] : kMapping) {
    if (value == v) {
      return name.str();
    }
  }
  return "<unknown>";
}

} // namespace facebook::eden
