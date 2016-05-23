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
#include "LocalMercurialRepoAndRev.h"
#include "eden/fuse/Inodes.h"

namespace facebook {
namespace eden {

// Represents a file from the hg manifest as an inode
class MercurialManifestFileInode : public fusell::FileInode {
  std::shared_ptr<LocalMercurialRepoAndRev> repo_;
  fuse_ino_t ino_, parent_;
  RelativePath path_;

 public:
  MercurialManifestFileInode(
      std::shared_ptr<LocalMercurialRepoAndRev> repo,
      fuse_ino_t ino,
      fuse_ino_t parent,
      RelativePathPiece path);
  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  folly::Future<std::string> readlink() override;
  folly::Future<std::unique_ptr<fusell::FileHandle>> open(
      const struct fuse_file_info& fi) override;
};
}
}
