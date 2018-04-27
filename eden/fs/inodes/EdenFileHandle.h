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
   * Construct an EdenFileHandle object.
   *
   * This should only be called by FileInode.  The caller is responsible for
   * acquiring an open refcount on the FileInode before constructing an
   * EdenFileHandle object.  The EdenFileHandle destructor will decrement the
   * FileInode's open refcount.
   *
   * *callerHasRefcount will be set to false by this constructor.  This makes
   * it easier for the caller to correctly decide if they still own the
   * refcount or not in case allocating the EdenFileHandle object throws an
   * exception.
   */
  explicit EdenFileHandle(FileInodePtr inode, bool* callerHasRefcount)
      : inode_(std::move(inode)) {
    *callerHasRefcount = false;
  }

  /**
   * EdenFileHandle destructor.
   *
   * This calls fileHandleDidClose on the associated inode to decrement its
   * open count.  Beware that fileHandleDidClose() acquires the FileInode lock,
   * so callers must ensure that EdenFileHandle objects can never be destroyed
   * while they are already holding an inode lock.
   */
  ~EdenFileHandle() override;

  InodeNumber getInodeNumber() override;
  folly::Future<Dispatcher::Attr> getattr() override;
  FOLLY_NODISCARD folly::Future<Dispatcher::Attr> setattr(
      const fuse_setattr_in& attr) override;
  bool preserveCache() const override;
  bool isSeekable() const override;
  folly::Future<BufVec> read(size_t size, off_t off) override;

  FOLLY_NODISCARD folly::Future<size_t> write(BufVec&& buf, off_t off) override;
  FOLLY_NODISCARD folly::Future<size_t> write(
      folly::StringPiece data,
      off_t off) override;
  FOLLY_NODISCARD folly::Future<folly::Unit> flush(
      uint64_t lock_owner) override;
  FOLLY_NODISCARD folly::Future<folly::Unit> fsync(bool datasync) override;

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
