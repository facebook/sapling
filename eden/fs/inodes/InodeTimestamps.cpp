/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/InodeTimestamps.h"

#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/utils/Clock.h"

namespace facebook {
namespace eden {

void InodeTimestamps::setattrTimes(
    const Clock& clock,
    const fuse_setattr_in& attr) {
  const auto now = clock.getRealtime();

  // Set atime for TreeInode.
  if (attr.valid & FATTR_ATIME) {
    atime.tv_sec = attr.atime;
    atime.tv_nsec = attr.atimensec;
  } else if (attr.valid & FATTR_ATIME_NOW) {
    atime = now;
  }

  // Set mtime for TreeInode.
  if (attr.valid & FATTR_MTIME) {
    mtime.tv_sec = attr.mtime;
    mtime.tv_nsec = attr.mtimensec;
  } else if (attr.valid & FATTR_MTIME_NOW) {
    mtime = now;
  }

  // we do not allow users to set ctime using setattr. ctime should be changed
  // when ever setattr is called, as this function is called in setattr, update
  // ctime to now.
  ctime = now;
}

} // namespace eden
} // namespace facebook
