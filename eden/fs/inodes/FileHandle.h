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

class FileHandle : public fusell::FileHandle {
 public:
  /**
   * The caller is responsible for incrementing any reference counts in the
   * given function.  This constructor does nothing but retain the specified
   * inode.
   *
   * Note that, for exception safety, the given function has to run during
   * FileHandle construction - if it throws, we don't want ~FileHandle to call
   * fileHandleDidClose.
   */
  template <typename Func>
  explicit FileHandle(FileInodePtr inode, Func&& func)
      : inode_(std::move(inode)) {
    func();
  }

  // Calls fileHandleDidClose on the associated inode.
  ~FileHandle() override;

  fusell::InodeNumber getInodeNumber() override;
  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  folly::Future<fusell::Dispatcher::Attr> setattr(
      const fuse_setattr_in& attr) override;
  bool preserveCache() const override;
  bool isSeekable() const override;
  folly::Future<fusell::BufVec> read(size_t size, off_t off) override;

  folly::Future<size_t> write(fusell::BufVec&& buf, off_t off) override;
  folly::Future<size_t> write(folly::StringPiece data, off_t off) override;
  folly::Future<folly::Unit> flush(uint64_t lock_owner) override;
  folly::Future<folly::Unit> fsync(bool datasync) override;

 private:
  FileHandle() = delete;
  FileHandle(const FileHandle&) = delete;
  FileHandle(FileHandle&&) = delete;
  FileHandle& operator=(const FileHandle&) = delete;
  FileHandle& operator=(FileHandle&&) = delete;

  FileInodePtr inode_;
};
} // namespace eden
} // namespace facebook
