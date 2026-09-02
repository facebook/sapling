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
 * How EdenFS responds to NFS requests claiming a privileged identity class
 * (root or the wheel group). See nfs:root-access-mode and
 * nfs:wheel-access-mode in EdenConfig.
 */
enum class NfsAccessMode {
  /**
   * Do nothing for this identity class.
   */
  Off,
  /**
   * Bump the class's nfs.privileged_access.* counter.
   */
  Log,
  /**
   * Log as above, and additionally bump nfs.blocked_access and reject the
   * request with an auth error.
   */
  Block,
};

template <>
class FieldConverter<NfsAccessMode> {
 public:
  folly::Expected<NfsAccessMode, std::string> fromString(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(NfsAccessMode value) const;
};

} // namespace facebook::eden
