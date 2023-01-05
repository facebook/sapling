/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/FieldConverter.h"

namespace facebook::eden {

enum class ReaddirPrefetch {
  None,
  Files,
  Trees,
  Both,
};

template <>
class FieldConverter<ReaddirPrefetch> {
 public:
  folly::Expected<ReaddirPrefetch, std::string> fromString(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(ReaddirPrefetch value) const;
};

} // namespace facebook::eden
