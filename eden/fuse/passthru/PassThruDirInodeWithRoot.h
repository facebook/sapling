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

#include "PassThruDirInode.h"

namespace facebook {
namespace eden {
namespace fusell {

class MountPoint;

class PassThruDirInodeWithRoot : public PassThruDirInode {
  AbsolutePath localRoot_;

 public:
  PassThruDirInodeWithRoot(
      MountPoint* mp,
      AbsolutePathPiece localRoot,
      fuse_ino_t ino,
      fuse_ino_t parent);

  AbsolutePath getLocalPath() const override;
};
}
}
}
