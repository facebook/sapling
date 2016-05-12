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

#include "eden/fuse/DirInode.h"

namespace facebook {
namespace eden {
namespace fusell {

class MountPoint;

class PassThruDirInode : public DirInode {
  MountPoint* const mount_{nullptr};
  fuse_ino_t const ino_;
  fuse_ino_t const parent_;

 public:
  PassThruDirInode(MountPoint* mp, fuse_ino_t ino, fuse_ino_t parent);

  static AbsolutePath getLocalPassThruInodePath(MountPoint* mp, fuse_ino_t ino);

  fuse_ino_t getFuseInode() const {
    return ino_;
  }
  fuse_ino_t getFuseParentInode() const {
    return parent_;
  }
  MountPoint* getMountPoint() const {
    return mount_;
  }

  virtual AbsolutePath getLocalPath() const;
  folly::Future<Dispatcher::Attr> getattr() override;
  folly::Future<DirHandle*> opendir(const struct fuse_file_info& fi) override;
  folly::Future<std::shared_ptr<InodeBase>> getChildByName(
      PathComponentPiece name) override;
  folly::Future<fuse_entry_param>
  mknod(PathComponentPiece name, mode_t mode, dev_t rdev) override;
  folly::Future<fuse_entry_param> mkdir(PathComponentPiece name, mode_t mode)
      override;
  folly::Future<folly::Unit> unlink(PathComponentPiece name) override;
  folly::Future<folly::Unit> rmdir(PathComponentPiece name) override;
  folly::Future<fuse_entry_param> symlink(
      PathComponentPiece link,
      PathComponentPiece name) override;
  folly::Future<folly::Unit> rename(
      PathComponentPiece name,
      std::shared_ptr<DirInode> newparent,
      PathComponentPiece newname) override;
  folly::Future<DirInode::CreateResult>
  create(PathComponentPiece name, mode_t mode, int flags) override;
  folly::Future<folly::Unit> setxattr(folly::StringPiece name,
                                      folly::StringPiece value,
                                      int flags) override;
  folly::Future<std::string> getxattr(folly::StringPiece name) override;
  folly::Future<std::vector<std::string>> listxattr() override;
  folly::Future<folly::Unit> removexattr(folly::StringPiece name) override;
};
}
}
}
