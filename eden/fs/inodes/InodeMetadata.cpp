/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeMetadata.h"
#include "eden/fs/fuse/FuseTypes.h"

namespace facebook {
namespace eden {

void InodeMetadata::updateFromAttr(
    const Clock& clock,
    const fuse_setattr_in& attr) {
  if (attr.valid & FATTR_MODE) {
    // Make sure we preserve the file type bits, and only update
    // permissions.
    mode = (mode & S_IFMT) | (07777 & attr.mode);
  }

  if (attr.valid & FATTR_UID) {
    uid = attr.uid;
  }
  if (attr.valid & FATTR_GID) {
    gid = attr.gid;
  }

  timestamps.setattrTimes(clock, attr);
}

void InodeMetadata::applyToStat(struct stat& st) const {
  st.st_mode = mode;
  st.st_uid = uid;
  st.st_gid = gid;
  timestamps.applyToStat(st);
}

} // namespace eden
} // namespace facebook
