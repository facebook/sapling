/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <sys/stat.h>
#include "eden/fs/inodes/InodeTimestamps.h"

struct fuse_setattr_in;
struct stat;

namespace facebook {
namespace eden {

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

  void updateFromAttr(const Clock& clock, const fuse_setattr_in& attr);

  void applyToStat(struct stat& st) const;

  // Other potential things to include:
  // nlink_t nlinks;
  // dev_t rdev;
  // creation time
};
} // namespace eden
} // namespace facebook
