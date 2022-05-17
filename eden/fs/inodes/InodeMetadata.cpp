/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/InodeMetadata.h"
#include "eden/fs/utils/FsChannelTypes.h"

namespace facebook::eden {

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

bool InodeMetadata::shouldShortCircuitMetadataUpdate(
    const DesiredMetadata& desired) {
  if (desired.size.has_value()) {
    return false;
  }
  // Note we only update permission bits, so we only check the equivalence of
  // those bits.
  if (desired.mode.has_value() &&
      (07777 & desired.mode.value()) != (07777 & mode)) {
    return false;
  }
  if (desired.uid.has_value() && desired.uid != uid) {
    return false;
  }
  if (desired.gid.has_value() && desired.gid != gid) {
    return false;
  }
  if (desired.atime.has_value() && !(desired.atime == timestamps.atime)) {
    return false;
  }
  if (desired.mtime.has_value() && !(desired.mtime == timestamps.mtime)) {
    return false;
  }

  return true;
}

void InodeMetadata::applyToStat(struct stat& st) const {
  st.st_mode = mode;
  st.st_uid = uid;
  st.st_gid = gid;
  timestamps.applyToStat(st);
}

} // namespace facebook::eden

#endif
