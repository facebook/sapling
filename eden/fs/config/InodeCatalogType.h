/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/FieldConverter.h"

namespace facebook::eden {

enum class InodeCatalogType : uint8_t {
  Legacy = 0,
  Sqlite = 1,
  InMemory = 2,
  LMDB = 3,
};

constexpr InodeCatalogType kInodeCatalogTypeDefault =
    folly::kIsWindows ? InodeCatalogType::Sqlite : InodeCatalogType::Legacy;

folly::Expected<InodeCatalogType, std::string> inodeCatalogTypeFromString(
    std::string_view value);

template <>
class FieldConverter<InodeCatalogType> {
 public:
  folly::Expected<InodeCatalogType, std::string> fromString(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(InodeCatalogType value) const;
};

} // namespace facebook::eden
