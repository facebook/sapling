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
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <atomic>
#include <memory>
#include <vector>
#include "eden/fuse/Dispatcher.h"
#include "eden/fuse/fuse_headers.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class TreeInode;

class InodeBase : public std::enable_shared_from_this<InodeBase> {
 public:
  InodeBase(
      fuse_ino_t ino,
      std::shared_ptr<TreeInode> parent,
      PathComponentPiece name);
  virtual ~InodeBase();

  fuse_ino_t getNodeId() const {
    return ino_;
  }

  void incNumLookups(uint32_t count = 1) {
    nlookup_.fetch_add(count, std::memory_order_acq_rel);
  }
  uint32_t decNumLookups(uint32_t count = 1) {
    auto prev = nlookup_.fetch_sub(count, std::memory_order_acq_rel);
    return prev - count;
  }

  // See Dispatcher::getattr
  virtual folly::Future<fusell::Dispatcher::Attr> getattr();

  // See Dispatcher::setattr
  virtual folly::Future<fusell::Dispatcher::Attr> setattr(
      const struct stat& attr,
      int to_set);

  virtual folly::Future<folly::Unit> setxattr(folly::StringPiece name,
                                              folly::StringPiece value,
                                              int flags);
  virtual folly::Future<std::string> getxattr(folly::StringPiece name);
  virtual folly::Future<std::vector<std::string>> listxattr();
  virtual folly::Future<folly::Unit> removexattr(folly::StringPiece name);
  virtual folly::Future<folly::Unit> access(int mask);

  /** Return true if Dispatcher should honor a FORGET and free
   * this inode object.  Return false if we should preserve it anyway. */
  virtual bool canForget();

  /**
   * Compute the path to this inode, from the root of the mount point.
   *
   * This will return the path to the file, or folly::none if the file has
   * been unlinked.
   *
   * BEWARE: Unless you are holding the mount-point's global rename lock when
   * you call this function, the file may have been renamed or unlinked by the
   * time you actually use the return value.
   */
  folly::Optional<RelativePath> getPath() const;

  /**
   * Get a string to use to refer to this file in a log message.
   *
   * This will usually return the path to the file, but if the file has been
   * unlinked it will return a string with data about where the file used to
   * exist.  The result is human-readable and is not designed for consumption
   * or parsing by other code.
   */
  std::string getLogPath() const;

 private:
  struct LocationInfo {
    LocationInfo(std::shared_ptr<TreeInode> p, PathComponentPiece n)
        : parent(std::move(p)), name(n) {}

    std::shared_ptr<TreeInode> parent;
    /**
     * unlinked will be set to true if the Inode has been unlinked from the
     * filesystem.
     *
     * The Inode object may continue to exist for some time after being
     * unlinked, but it can no longer be referred to by name.  For example, the
     * Inode object will continue to exist for at least as long as there are
     * open file handles referring to it.
     *
     * The name member will still track the file's old name, but it should only
     * be used for debugging/logging purposes at that point.
     */
    bool unlinked{false};
    PathComponent name;
  };

  bool getPathHelper(std::vector<PathComponent>& names, bool stopOnUnlinked)
      const;

  fuse_ino_t const ino_;
  // A reference count tracking the outstanding lookups that the kernel
  // has performed on this inode.  This lets us track when we can forget
  // about it.
  std::atomic<uint32_t> nlookup_{1};

  /**
   * Information about this Inode's location in the file system path.
   * Eden does not support hard links, so each Inode has exactly one location.
   *
   * To read the location data you only need to acquire the Synchronized
   * object's read lock.
   *
   * However, to update location data you must acquire both the mount point's
   * global rename lock and acquire this Synchronized object's write lock.
   * (acquire the mount-point rename lock first).
   *
   * TODO: The mount point rename lock does not exist yet.  We need to add it
   * in a future diff, and update rename() and unlink() operations to always
   * hold it before updating location data.  Currently rename() and unlink()
   * don't ever update parent pointers or names yet.
   */
  folly::Synchronized<LocationInfo> location_;
};
}
}
