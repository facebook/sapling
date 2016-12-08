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
#include <folly/SharedMutex.h>
#include <unordered_map>
#include "eden/fuse/Dispatcher.h"

namespace facebook {
namespace eden {

namespace fusell {
class InodeNameManager;
}

class EdenMount;
class FileInode;
class InodeBase;
class TreeInode;

/**
 * A FUSE request dispatcher for eden mount points.
 */
class EdenDispatcher : public fusell::Dispatcher {
  std::shared_ptr<TreeInode> root_;
  std::unordered_map<fuse_ino_t, std::shared_ptr<InodeBase>> inodes_;
  mutable folly::SharedMutex lock_;

  // The EdenMount that owns this EdenDispatcher.
  EdenMount* const mount_;

 public:
  /*
   * Create an EdenDispatcher, without a root node yet.
   * setRootInode() must be called before using this dispatcher.
   */
  explicit EdenDispatcher(EdenMount* mount);

  /*
   * Create an EdenDispatcher using the specified root inode object.
   */
  explicit EdenDispatcher(
      EdenMount* mount,
      std::shared_ptr<TreeInode> rootInode);

  std::shared_ptr<InodeBase> getInode(fuse_ino_t, bool mustExist = true) const;
  std::shared_ptr<InodeBase> lookupInode(fuse_ino_t) const;
  std::shared_ptr<TreeInode> getTreeInode(fuse_ino_t, bool mustExist = true)
      const;
  std::shared_ptr<FileInode> getFileInode(fuse_ino_t, bool mustExist = true)
      const;

  /*
   * Set the root inode.
   *
   * This method should be used to set the root inode on a default-constructed
   * EdenDispatcher.  It may only be called once, and it must be called before
   * using the EdenDispatcher.
   */
  void setRootInode(std::shared_ptr<TreeInode> inode);

  /** Throws if setRootInode() has not been invoked yet. */
  std::shared_ptr<TreeInode> getRootInode() const;

  void recordInode(std::shared_ptr<InodeBase> inode);

  void initConnection(fuse_conn_info& conn) override;
  folly::Future<Attr> getattr(fuse_ino_t ino) override;
  folly::Future<Attr> setattr(fuse_ino_t ino,
                              const struct stat& attr,
                              int to_set) override;
  folly::Future<std::shared_ptr<fusell::DirHandle>> opendir(
      fuse_ino_t ino,
      const struct fuse_file_info& fi) override;
  folly::Future<fuse_entry_param> lookup(
      fuse_ino_t parent,
      PathComponentPiece name) override;

  /**
   * Similar to lookup(), except this does not require an active FUSE request.
   */
  folly::Future<std::shared_ptr<InodeBase>> lookupInodeBase(
      fuse_ino_t parent,
      PathComponentPiece name);
  folly::Future<folly::Unit> forget(fuse_ino_t ino,
                                    unsigned long nlookup) override;
  folly::Future<std::shared_ptr<fusell::FileHandle>> open(
      fuse_ino_t ino,
      const struct fuse_file_info& fi) override;
  folly::Future<std::string> readlink(fuse_ino_t ino) override;
  folly::Future<fuse_entry_param> mknod(
      fuse_ino_t parent,
      PathComponentPiece name,
      mode_t mode,
      dev_t rdev) override;
  folly::Future<fuse_entry_param>
  mkdir(fuse_ino_t parent, PathComponentPiece name, mode_t mode) override;
  folly::Future<folly::Unit> unlink(fuse_ino_t parent, PathComponentPiece name)
      override;
  folly::Future<folly::Unit> rmdir(fuse_ino_t parent, PathComponentPiece name)
      override;
  folly::Future<fuse_entry_param> symlink(
      PathComponentPiece link,
      fuse_ino_t parent,
      PathComponentPiece name) override;
  folly::Future<folly::Unit> rename(
      fuse_ino_t parent,
      PathComponentPiece name,
      fuse_ino_t newparent,
      PathComponentPiece newname) override;

  folly::Future<fuse_entry_param> link(
      fuse_ino_t ino,
      fuse_ino_t newparent,
      PathComponentPiece newname) override;

  folly::Future<Dispatcher::Create> create(
      fuse_ino_t parent,
      PathComponentPiece name,
      mode_t mode,
      int flags) override;
  folly::Future<std::string> getxattr(fuse_ino_t ino, folly::StringPiece name)
      override;
  folly::Future<std::vector<std::string>> listxattr(fuse_ino_t ino) override;
};
}
}
