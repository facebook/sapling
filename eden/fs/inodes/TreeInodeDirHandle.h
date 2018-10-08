/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/inodes/InodePtr.h"

namespace facebook {
namespace eden {

class TreeInodeDirHandle : public DirHandle {
 public:
  explicit TreeInodeDirHandle(TreeInodePtr inode);

  folly::Future<DirList> readdir(DirList&& list, off_t off) override;

  folly::Future<folly::Unit> fsyncdir(bool datasync) override;

 private:
  TreeInodePtr inode_;
};
} // namespace eden
} // namespace facebook
