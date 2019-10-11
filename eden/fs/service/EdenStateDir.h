/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/Range.h>

#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class File;
}

namespace facebook {
namespace eden {

/**
 * EdenStateDir exists for managing access to the user's .eden directory.
 *
 * Note that this refers to the user's main .eden directory where Eden stores
 * its state, and not the virtual .eden directories that appear in all mounted
 * Eden checkouts.
 */
class EdenStateDir {
 public:
  explicit EdenStateDir(AbsolutePathPiece path);
  ~EdenStateDir();

  /**
   * Acquire the main on-disk edenfs lock.
   *
   * Callers should acquire the on-disk lock before performing any other
   * operations on the EdenStateDir, to ensure that only one process can use the
   * state directory at a time.
   *
   * Returns true if the lock was acquired successfully, or false if we failed
   * to acquire the lock (likely due to another process holding it).
   * May throw an exception on other errors (e.g., insufficient permissions to
   * create the lock file, out of disk space, etc).
   */
  FOLLY_NODISCARD bool acquireLock();

  /**
   * Take over the lock file from another process.
   */
  void takeoverLock(folly::File lockFile);

  /**
   * Extract the lock file without releasing it.
   *
   * This is primarily intended to be used to transfer the lock to another
   * process.  This file descriptor can be transferred to the other process,
   * which will then pass it to the takeoverLock() method of their EdenStateDir
   * object.
   */
  folly::File extractLock();

  /**
   * Returns true if the Eden state directory lock is currently held by this
   * EdenStateDir object.
   */
  bool isLocked() const;

  /**
   * Get the path to the state directory.
   */
  AbsolutePathPiece getPath() const {
    return path_;
  }

  /**
   * Get the path to Eden's thrift socket.
   */
  AbsolutePath getThriftSocketPath() const;

  /**
   * Get the path to Eden's takeover socket.
   */
  AbsolutePath getTakeoverSocketPath() const;

  /**
   * Get the path to the directory where state for a specific checkout is
   * stored.
   *
   * Note that the checkoutID string must meet the requirements of
   * PathComponent: it must not contain internal directory separators and must
   * not be "." or "..".
   */
  AbsolutePath getCheckoutStateDir(folly::StringPiece checkoutID) const;

 private:
  static void writePidToLockFile(folly::File& lockFile);

  AbsolutePath path_;
  folly::File lockFile_;
};
} // namespace eden
} // namespace facebook
