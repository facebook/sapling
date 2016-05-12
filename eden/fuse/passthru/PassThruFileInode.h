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

#include "eden/fuse/FileInode.h"

namespace facebook {
namespace eden {
namespace fusell {

class MountPoint;

class PassThruFileInode : public FileInode {
  MountPoint* const mount_{nullptr};
  fuse_ino_t ino_;
  fuse_ino_t parent_;

 public:
  explicit PassThruFileInode(MountPoint* mp, fuse_ino_t ino, fuse_ino_t parent);
  virtual AbsolutePath getLocalPath() const;
  folly::Future<Dispatcher::Attr> getattr() override;
  folly::Future<FileHandle*> open(const struct fuse_file_info& fi) override;
  folly::Future<std::string> readlink() override;
  folly::Future<folly::Unit> setxattr(folly::StringPiece name,
                                      folly::StringPiece value,
                                      int flags) override;
  folly::Future<std::string> getxattr(folly::StringPiece name) override;
  folly::Future<std::vector<std::string>> listxattr() override;
  folly::Future<folly::Unit> removexattr(folly::StringPiece name) override;
  folly::Future<fuse_entry_param> link(
      std::shared_ptr<DirInode> newparent,
      PathComponentPiece newname) override;
};
}
}
}
