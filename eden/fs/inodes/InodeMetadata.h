/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sys/stat.h>
#include <optional>
#include "eden/fs/inodes/InodeTimestamps.h"

struct stat;

namespace facebook::eden {

/**
 * Set of metadata to update during an InodeBase::setattr call.
 *
 * Any non-optional field will be reflected into the corresponding
 * InodeMetadata object.
 */
struct DesiredMetadata {
  std::optional<size_t> size;
  std::optional<mode_t> mode;
  std::optional<uid_t> uid;
  std::optional<gid_t> gid;
  std::optional<timespec> atime;
  std::optional<timespec> mtime;

  bool is_nop(bool ignoreAtime) const {
    // `ignoreAtime` exists, so that it can be ignored for scenarios
    // where `atime` is not supported (e.g., higher-level NFS functions)
    // but setters can still work internally.
    return !size.has_value() && !mode.has_value() && !uid.has_value() &&
        !gid.has_value() && !mtime.has_value() &&
        (!atime.has_value() || ignoreAtime);
  }
};

/**
 * Fixed-size structure of per-inode bits that should be persisted across runs.
 *
 * Warning: This data structure is serialized directly to disk via InodeTable.
 * Do not change the order, sizes, or meanings of the fields. Instead, rename
 * this struct, create a new InodeMetadata struct with the next VERSION value,
 * add an explicit constructor from the old version, and add the old version to
 * the InodeMetadataTable typedef in InodeTable.h.
 */
struct InodeMetadata {
  enum { VERSION = 0 };

  InodeMetadata() = default;

  explicit InodeMetadata(
      mode_t m,
      uid_t u,
      gid_t g,
      const InodeTimestamps& ts) noexcept
      : mode{m}, uid{u}, gid{g}, timestamps{ts} {}

  mode_t mode{0};
  uid_t uid{0};
  gid_t gid{0};
  InodeTimestamps timestamps;

  void updateFromDesired(const Clock& clock, const DesiredMetadata& attr);

  /**
   * Checks if the desired metadata is the same as the current metadata,
   * allowing us to skip updating the metadata.
   */
  bool shouldShortCircuitMetadataUpdate(const DesiredMetadata& desired);

  void applyToStat(struct stat& st) const;

  // Other potential things to include:
  // nlink_t nlinks;
  // dev_t rdev;
  // creation time
};

} // namespace facebook::eden
