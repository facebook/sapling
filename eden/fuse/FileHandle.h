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
#include "FileHandleBase.h"

namespace facebook {
namespace eden {
namespace fusell {

class FileHandle : public FileHandleBase {
 public:
  /**
   * Return true if this file handle uses direct IO
   */
  virtual bool usesDirectIO() const;

  /**
   * Return true if, at open() time, the kernel can retain cached info.
   */
  virtual bool preserveCache() const;

  /**
   * Return true if the file is seekable.
   */
  virtual bool isSeekable() const;

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
  virtual folly::Future<size_t> write(BufVec&& buf, off_t off) = 0;
  virtual folly::Future<size_t> write(folly::StringPiece data, off_t off) = 0;

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
  virtual folly::Future<folly::Unit> flush(uint64_t lock_owner) = 0;

  /**
   * Release an open file
   *
   * Release is called when there are no more references to an open
   * file: all file descriptors are closed and all memory mappings
   * are unmapped.
   *
   * For every open call there will be exactly one release call.
   *
   * The filesystem may reply with an error, but error values are
   * not returned to close() or munmap() which triggered the
   * release.
   */
  virtual folly::Future<folly::Unit> release();

  /**
   * Synchronize file contents
   *
   * If the datasync parameter is non-zero, then only the user data
   * should be flushed, not the meta data.
   *
   * @param datasync flag indicating if only data should be flushed
   * @param fi file information
   */
  virtual folly::Future<folly::Unit> fsync(bool datasync) = 0;

  /**
   * Test for a POSIX file lock
   *
   * Introduced in version 2.6
   *
   * @param ino the inode number
   * @param fi file information
   * @param lock the region/type to test
   */
  virtual folly::Future<struct flock> getlk(struct flock lock,
                                            uint64_t lock_owner);

  /**
   * Acquire, modify or release a POSIX file lock
   *
   * For POSIX threads (NPTL) there's a 1-1 relation between pid and
   * owner, but otherwise this is not always the case.  For checking
   * lock ownership, 'fi->owner' must be used.  The l_pid field in
   * 'struct flock' should only be used to fill in this field in
   * getlk().
   *
   * Note: if the locking methods are not implemented, the kernel
   * will still allow file locking to work locally.  Hence these are
   * only interesting for network filesystems and similar.
   *
   * Introduced in version 2.6
   *
   * Valid replies:
   *   fuse_reply_err
   *
   * @param req request handle
   * @param ino the inode number
   * @param fi file information
   * @param lock the region/type to test
   * @param sleep locking operation may sleep
   */
  virtual folly::Future<folly::Unit> setlk(struct flock lock,
                                           bool sleep,
                                           uint64_t lock_owner);
};
}
}
}
