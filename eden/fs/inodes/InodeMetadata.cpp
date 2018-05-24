/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
