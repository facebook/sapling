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
#include "eden/fs/fuse/FileHandleBase.h"

namespace facebook {
namespace eden {

class FileHandle : public FileHandleBase {
 public:
  /**
   * Read data
   *
   * Read should send exactly the number of bytes requested except
   * on EOF or error, otherwise the rest of the data will be
   * substituted with zeroes.  An exception to this is when the file
   * has been opened in 'direct_io' mode, in which case the return
   * value of the read system call will reflect the return value of
   * this operation.
   *
   * @param size number of bytes to read
   * @param off offset to read from
   */
  virtual folly::Future<BufVec> read(size_t size, off_t off) = 0;

  /**
   * Write data
   *
   * Write should return exactly the number of bytes requested
   * except on error.  An exception to this is when the file has
   * been opened in 'direct_io' mode, in which case the return value
   * of the write system call will reflect the return value of this
   * operation.
   */
  FOLLY_NODISCARD virtual folly::Future<size_t> write(
      BufVec&& buf,
      off_t off) = 0;
  FOLLY_NODISCARD virtual folly::Future<size_t> write(
      folly::StringPiece data,
      off_t off) = 0;

  /**
   * Flush method
   *
   * This is called on each close() of the opened file.
   *
   * Since file descriptors can be duplicated (dup, dup2, fork), for
   * one open call there may be many flush calls.
   *
   * Filesystems shouldn't assume that flush will always be called
   * after some writes, or that if will be called at all.
   *
   * NOTE: the name of the method is misleading, since (unlike
   * fsync) the filesystem is not forced to flush pending writes.
   * One reason to flush data, is if the filesystem wants to return
   * write errors.
   *
   * If the filesystem supports file locking operations (setlk,
   * getlk) it should remove all locks belonging to 'lock_owner'.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> flush(
      uint64_t lock_owner) = 0;

  /**
   * Synchronize file contents
   *
   * If the datasync parameter is non-zero, then only the user data
   * should be flushed, not the meta data.
   *
   * @param datasync flag indicating if only data should be flushed
   * @param fi file information
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> fsync(bool datasync) = 0;
};

} // namespace eden
} // namespace facebook
