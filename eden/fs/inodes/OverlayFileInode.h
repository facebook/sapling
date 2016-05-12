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
#include "eden/fs/overlay/Overlay.h"
#include "eden/fuse/Inodes.h"
#include "eden/fuse/passthru/PassThruFileInode.h"

namespace facebook {
namespace eden {

/** An inode for a file stored in the overlay area */
class OverlayFileInode : public fusell::PassThruFileInode {
 public:
  OverlayFileInode(
      fusell::MountPoint* mountPoint,
      fuse_ino_t parent,
      fuse_ino_t ino,
      std::shared_ptr<Overlay> overlay);

  AbsolutePath getLocalPath() const override;

 private:
  std::shared_ptr<Overlay> overlay_;
};
}
}
