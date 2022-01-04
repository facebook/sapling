/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/InodeMetadata.h"
#include "eden/fs/utils/FsChannelTypes.h"

namespace facebook {
namespace eden {

void InodeMetadata::updateFromDesired(
    const Clock& clock,
    const DesiredMetadata& attr) {
  if (attr.mode.has_value()) {
    // Make sure we preserve the file type bits, and only update
    // permissions.
    mode = (mode & S_IFMT) | (07777 & attr.mode.value());
  }

  if (attr.uid.has_value()) {
    uid = attr.uid.value();
  }
  if (attr.gid.has_value()) {
    gid = attr.gid.value();
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

#endif
