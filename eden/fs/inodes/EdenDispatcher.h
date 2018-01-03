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
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/inodes/InodePtr.h"

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
  void onConnectionReady() override;

  folly::Future<Attr> getattr(fusell::InodeNumber ino) override;
  folly::Future<Attr> setattr(
      fusell::InodeNumber ino,
      const fuse_setattr_in& attr) override;
  folly::Future<std::shared_ptr<fusell::DirHandle>> opendir(
      fusell::InodeNumber ino,
      int flags) override;
  folly::Future<fuse_entry_out> lookup(
      fusell::InodeNumber parent,
      PathComponentPiece name) override;

  folly::Future<folly::Unit> forget(
      fusell::InodeNumber ino,
      unsigned long nlookup) override;
  folly::Future<std::shared_ptr<fusell::FileHandle>> open(
      fusell::InodeNumber ino,
      int flags) override;
  folly::Future<std::string> readlink(fusell::InodeNumber ino) override;
  folly::Future<fuse_entry_out> mknod(
      fusell::InodeNumber parent,
      PathComponentPiece name,
      mode_t mode,
      dev_t rdev) override;
  folly::Future<fuse_entry_out> mkdir(
      fusell::InodeNumber parent,
      PathComponentPiece name,
      mode_t mode) override;
  folly::Future<folly::Unit> unlink(
      fusell::InodeNumber parent,
      PathComponentPiece name) override;
  folly::Future<folly::Unit> rmdir(
      fusell::InodeNumber parent,
      PathComponentPiece name) override;
  folly::Future<fuse_entry_out> symlink(
      fusell::InodeNumber parent,
      PathComponentPiece name,
      folly::StringPiece link) override;
  folly::Future<folly::Unit> rename(
      fusell::InodeNumber parent,
      PathComponentPiece name,
      fusell::InodeNumber newparent,
      PathComponentPiece newname) override;

  folly::Future<fuse_entry_out> link(
      fusell::InodeNumber ino,
      fusell::InodeNumber newparent,
      PathComponentPiece newname) override;

  folly::Future<Dispatcher::Create> create(
      fusell::InodeNumber parent,
      PathComponentPiece name,
      mode_t mode,
      int flags) override;
  folly::Future<std::string> getxattr(
      fusell::InodeNumber ino,
      folly::StringPiece name) override;
  folly::Future<std::vector<std::string>> listxattr(
      fusell::InodeNumber ino) override;

 private:
  // The EdenMount that owns this EdenDispatcher.
  EdenMount* const mount_;
  // The EdenMount's InodeMap.
  // We store this pointer purely for convenience.  We need it on pretty much
  // every FUSE request, and having it locally avoids  having to dereference
  // mount_ first.
  InodeMap* const inodeMap_;
};
} // namespace eden
} // namespace facebook
