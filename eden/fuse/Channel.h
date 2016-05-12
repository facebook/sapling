/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Range.h>
#include "eden/fuse/fuse_headers.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {
namespace fusell {

class Dispatcher;
class MountPoint;

class Channel {
  fuse_chan* ch_;
  const MountPoint* mountPoint_;

  friend class SessionDeleter;

 public:
  Channel(const Channel&) = delete;
  Channel& operator=(const Channel&) = delete;
  Channel(Channel&&) = default;
  Channel& operator=(Channel&&) = default;

  explicit Channel(const MountPoint* mountPoint);
  ~Channel();

  const MountPoint* getMountPoint() const;

  void runSession(Dispatcher* disp, bool debug);

  /**
   * Notify to invalidate cache for an inode
   *
   * @param ino the inode number
   * @param off the offset in the inode where to start invalidating
   *            or negative to invalidate attributes only
   * @param len the amount of cache to invalidate or 0 for all
   */
  void invalidateInode(fuse_ino_t ino, off_t off, off_t len);

  /**
   * Notify to invalidate parent attributes and the dentry matching
   * parent/name
   *
   * @param ch the channel through which to send the invalidation
   * @param parent inode number
   * @param name file name
   * @param namelen strlen() of file name
   * @return zero for success, -errno for failure
   */
  void invalidateEntry(fuse_ino_t parent, PathComponentPiece name);
};
}
}
}
