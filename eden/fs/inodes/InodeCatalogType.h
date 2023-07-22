/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/FieldConverter.h"

namespace facebook::eden {

/**
 * NOTE: We should consider revisiting this. Fundamentally,
 * there are three types: Legacy, Sqlite and InMemory. The remaining are flags
 * that control the Sqlite runtime. They could be moved to separated into flags
 * that are available for InodeCatalogs that support them.
 */
enum class InodeCatalogType : uint8_t {
  Legacy = 0,
  Sqlite = 1,
  SqliteInMemory = 2,
  SqliteSynchronousOff = 3,
  SqliteBuffered = 4,
  SqliteInMemoryBuffered = 5,
  SqliteSynchronousOffBuffered = 6,
  InMemory = 7,
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
