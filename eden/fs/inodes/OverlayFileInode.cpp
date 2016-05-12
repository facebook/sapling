/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "OverlayFileInode.h"

namespace facebook {
namespace eden {

OverlayFileInode::OverlayFileInode(
    fusell::MountPoint* mountPoint,
    fuse_ino_t parent,
    fuse_ino_t ino,
    std::shared_ptr<Overlay> overlay)
    : fusell::PassThruFileInode(mountPoint, ino, parent), overlay_(overlay) {}

AbsolutePath OverlayFileInode::getLocalPath() const {
  return overlay_->getLocalDir() +
      fusell::InodeNameManager::get()->resolvePathToNode(getNodeId());
}
}
}
