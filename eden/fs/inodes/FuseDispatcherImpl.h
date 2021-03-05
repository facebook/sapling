/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/fuse/FuseDispatcher.h"

namespace facebook::eden {

class EdenMount;
class InodeMap;

/**
 * Implement the FuseDispatcher interface.
 *
 * For unsupported operations, the corresponding methods are explicitly not
 * overridden and will directly fail in FuseDispatcher.
 */
class FuseDispatcherImpl : public FuseDispatcher {
 public:
  explicit FuseDispatcherImpl(EdenMount* mount);

  folly::Future<struct fuse_kstatfs> statfs(InodeNumber ino) override;
  folly::Future<Attr> getattr(InodeNumber ino, ObjectFetchContext& context)
      override;
  folly::Future<Attr> setattr(InodeNumber ino, const fuse_setattr_in& attr)
      override;
  folly::Future<uint64_t> opendir(InodeNumber ino, int flags) override;
  folly::Future<folly::Unit> releasedir(InodeNumber ino, uint64_t fh) override;
  folly::Future<fuse_entry_out> lookup(
      uint64_t requestID,
      InodeNumber parent,
      PathComponentPiece name,
      ObjectFetchContext& context) override;

  void forget(InodeNumber ino, unsigned long nlookup) override;
  folly::Future<uint64_t> open(InodeNumber ino, int flags) override;
  folly::Future<std::string> readlink(
      InodeNumber ino,
      bool kernelCachesReadlink) override;
  folly::Future<fuse_entry_out> mknod(
      InodeNumber parent,
      PathComponentPiece name,
      mode_t mode,
      dev_t rdev) override;
  folly::Future<fuse_entry_out>
  mkdir(InodeNumber parent, PathComponentPiece name, mode_t mode) override;
  folly::Future<folly::Unit> unlink(
      InodeNumber parent,
      PathComponentPiece name,
      ObjectFetchContext& context) override;
  folly::Future<folly::Unit> rmdir(
      InodeNumber parent,
      PathComponentPiece name,
      ObjectFetchContext& context) override;
  folly::Future<fuse_entry_out> symlink(
      InodeNumber parent,
      PathComponentPiece name,
      folly::StringPiece link) override;
  folly::Future<folly::Unit> rename(
      InodeNumber parent,
      PathComponentPiece name,
      InodeNumber newparent,
      PathComponentPiece newname) override;

  folly::Future<fuse_entry_out> link(
      InodeNumber ino,
      InodeNumber newparent,
      PathComponentPiece newname) override;

  folly::Future<fuse_entry_out> create(
      InodeNumber parent,
      PathComponentPiece name,
      mode_t mode,
      int flags) override;

  folly::Future<BufVec> read(
      InodeNumber ino,
      size_t size,
      off_t off,
      ObjectFetchContext& context) override;
  folly::Future<size_t>
  write(InodeNumber ino, folly::StringPiece data, off_t off) override;

  folly::Future<folly::Unit> flush(InodeNumber ino, uint64_t lock_owner)
      override;
  folly::Future<folly::Unit>
  fallocate(InodeNumber ino, uint64_t offset, uint64_t length) override;
  folly::Future<folly::Unit> fsync(InodeNumber ino, bool datasync) override;
  folly::Future<folly::Unit> fsyncdir(InodeNumber ino, bool datasync) override;

  folly::Future<DirList> readdir(
      InodeNumber ino,
      DirList&& dirList,
      off_t offset,
      uint64_t fh,
      ObjectFetchContext& context) override;

  folly::Future<std::string> getxattr(InodeNumber ino, folly::StringPiece name)
      override;
  folly::Future<std::vector<std::string>> listxattr(InodeNumber ino) override;

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;

  // The EdenMount's InodeMap.
  // We store this pointer purely for convenience. We need it on pretty much
  // every FUSE request, and having it locally avoids having to dereference
  // mount_ first.
  InodeMap* const inodeMap_;
};

} // namespace facebook::eden
