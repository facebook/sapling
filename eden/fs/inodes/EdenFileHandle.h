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
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/inodes/InodePtr.h"

namespace facebook {
namespace eden {

class Blob;
class FileInode;
class LocalStore;

class EdenFileHandle : public FileHandle {
 public:
  /**
   * The caller is responsible for incrementing any reference counts in the
   * given function.  This constructor does nothing but retain the specified
   * inode.
   *
   * Note that, for exception safety, the given function has to run during
   * EdenFileHandle construction - if it throws, we don't want ~EdenFileHandle
   * to call fileHandleDidClose.
   */
  template <typename Func>
  explicit EdenFileHandle(FileInodePtr inode, Func&& func)
      : inode_(std::move(inode)) {
    func();
  }

  // Calls fileHandleDidClose on the associated inode.
  ~EdenFileHandle() override;

  InodeNumber getInodeNumber() override;
  folly::Future<Dispatcher::Attr> getattr() override;
  folly::Future<Dispatcher::Attr> setattr(const fuse_setattr_in& attr) override;
  bool preserveCache() const override;
  bool isSeekable() const override;
  folly::Future<BufVec> read(size_t size, off_t off) override;

  folly::Future<size_t> write(BufVec&& buf, off_t off) override;
  folly::Future<size_t> write(folly::StringPiece data, off_t off) override;
  folly::Future<folly::Unit> flush(uint64_t lock_owner) override;
  folly::Future<folly::Unit> fsync(bool datasync) override;

 private:
  EdenFileHandle() = delete;
  EdenFileHandle(const EdenFileHandle&) = delete;
  EdenFileHandle(EdenFileHandle&&) = delete;
  EdenFileHandle& operator=(const EdenFileHandle&) = delete;
  EdenFileHandle& operator=(EdenFileHandle&&) = delete;

  FileInodePtr inode_;
};

} // namespace eden
} // namespace facebook
