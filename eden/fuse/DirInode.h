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
#include "FileHandle.h"
#include "InodeBase.h"
#include "InodeNameManager.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {
namespace fusell {

class DirInode : public InodeBase {
 public:
  explicit DirInode(fuse_ino_t ino);
  virtual folly::Future<std::shared_ptr<InodeBase>> getChildByName(
      PathComponentPiece name);

  virtual folly::Future<fuse_entry_param>
  mknod(PathComponentPiece name, mode_t mode, dev_t rdev);
  virtual folly::Future<fuse_entry_param> mkdir(
      PathComponentPiece name,
      mode_t mode);
  virtual folly::Future<folly::Unit> unlink(PathComponentPiece name);
  virtual folly::Future<folly::Unit> rmdir(PathComponentPiece name);
  virtual folly::Future<fuse_entry_param> symlink(
      PathComponentPiece link,
      PathComponentPiece name);
  virtual folly::Future<folly::Unit> rename(
      PathComponentPiece name,
      std::shared_ptr<DirInode> newparent,
      PathComponentPiece newname);
  virtual folly::Future<std::unique_ptr<DirHandle>> opendir(
      const struct fuse_file_info& fi);
  virtual folly::Future<struct statvfs> statfs();

  /** Holds the results of a create operation.
   *
   * It is important that the file handle creation respect O_EXCL if
   * it set in the flags parameter to DirInode::create.
   */
  struct CreateResult {
    /// file attributes and cache ttls.
    Dispatcher::Attr attr;
    /// The newly created inode instance.
    std::shared_ptr<InodeBase> inode;
    /// The newly opened file handle.
    std::unique_ptr<FileHandle> file;
    /// The newly created node record from the name manager.
    std::shared_ptr<InodeNameManager::Node> node;
  };

  virtual folly::Future<CreateResult>
  create(PathComponentPiece name, mode_t mode, int flags);
};
}
}
}
