/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/FieldConverter.h"

namespace facebook::eden {

enum class HgObjectIdFormat {
  /// 20 bytes that index into LocalStore's hgproxyhash keyspace
  ProxyHash,
  /// '1' followed by 20 bytes of hg manifest hash and then a path
  // WithPath,
  /// '2' followed by 20 bytes of hg manifest hash
  HashOnly,
};

template <>
class FieldConverter<HgObjectIdFormat> {
 public:
  folly::Expected<HgObjectIdFormat, std::string> fromString(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(HgObjectIdFormat value) const;
};

} // namespace facebook::eden
