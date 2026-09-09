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
 * What EdenFS does with an NFS request whose AUTH_SYS credential matches an
 * entry in nfs:uid-access-policy or nfs:gid-access-policy (see EdenConfig).
 */
enum class NfsAccessMode {
  /**
   * Bump the id's nfs.access.{uid,gid}.<id> stat.
   */
  Log,
  /**
   * Log as above, and for procedures in nfs:access-policy-procedures also bump
   * nfs.blocked.{uid,gid}.<id> and nfs.blocked_access and reject (auth error).
   */
  Block,
  /**
   * Log as above, but only reject (as Block does) while the id's policed
   * requests within nfs:access-policy-rate-limit-window-seconds exceed
   * nfs:access-policy-rate-limit-count — allow the low baseline, shed bursts.
   */
  RateLimit,
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
