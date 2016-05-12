/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Inodes.h"

using namespace folly;

namespace facebook {
namespace eden {
namespace fusell {

DirInode::DirInode(fuse_ino_t ino) : InodeBase(ino) {}

folly::Future<std::shared_ptr<InodeBase>> DirInode::getChildByName(
    PathComponentPiece name) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_param>
DirInode::mknod(PathComponentPiece name, mode_t mode, dev_t rdev) {
  FUSELL_NOT_IMPL();
}
folly::Future<fuse_entry_param> DirInode::mkdir(PathComponentPiece, mode_t) {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> DirInode::unlink(PathComponentPiece) {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> DirInode::rmdir(PathComponentPiece) {
  FUSELL_NOT_IMPL();
}
folly::Future<fuse_entry_param> DirInode::symlink(
    PathComponentPiece,
    PathComponentPiece) {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> DirInode::rename(
    PathComponentPiece,
    std::shared_ptr<DirInode>,
    PathComponentPiece) {
  FUSELL_NOT_IMPL();
}
folly::Future<DirHandle*> DirInode::opendir(const struct fuse_file_info& fi) {
  FUSELL_NOT_IMPL();
}
folly::Future<struct statvfs> DirInode::statfs() {
  FUSELL_NOT_IMPL();
}
folly::Future<DirInode::CreateResult>
DirInode::create(PathComponentPiece, mode_t, int) {
  FUSELL_NOT_IMPL();
}
}
}
}
