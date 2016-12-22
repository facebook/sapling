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
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fuse/Dispatcher.h"

namespace facebook {
namespace eden {

class EdenMount;
class FileInode;
class InodeBase;
class InodeMap;
class TreeInode;

/**
 * A FUSE request dispatcher for eden mount points.
 */
class EdenDispatcher : public fusell::Dispatcher {
 public:
  /*
   * Create an EdenDispatcher.
   * setRootInode() must be called before using this dispatcher.
   */
  explicit EdenDispatcher(EdenMount* mount);

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
      fuse_ino_t parent,
      PathComponentPiece name,
      folly::StringPiece link) override;
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

 private:
  // The EdenMount that owns this EdenDispatcher.
  EdenMount* const mount_;
  // The EdenMount's InodeMap.
  // We store this pointer purely for convenience.  We need it on pretty much
  // every FUSE request, and having it locally avoids  having to dereference
  // mount_ first.
  InodeMap* const inodeMap_;
};
}
}
