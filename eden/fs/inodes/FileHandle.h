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
#include "eden/fuse/FileHandle.h"

namespace facebook {
namespace eden {

class Blob;
class FileData;
class FileInode;
class LocalStore;

class FileHandle : public fusell::FileHandle {
 public:
  explicit FileHandle(
      FileInodePtr inode,
      std::shared_ptr<FileData> data,
      int flags);
  ~FileHandle();

  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  folly::Future<fusell::Dispatcher::Attr> setattr(
      const struct stat& attr,
      int to_set) override;
  bool preserveCache() const override;
  bool isSeekable() const override;
  folly::Future<fusell::BufVec> read(size_t size, off_t off) override;

  folly::Future<size_t> write(fusell::BufVec&& buf, off_t off) override;
  folly::Future<size_t> write(folly::StringPiece data, off_t off) override;
  folly::Future<folly::Unit> flush(uint64_t lock_owner) override;
  folly::Future<folly::Unit> fsync(bool datasync) override;

 private:
  FileInodePtr inode_;
  std::shared_ptr<FileData> data_;
  int openFlags_;
};
}
}
