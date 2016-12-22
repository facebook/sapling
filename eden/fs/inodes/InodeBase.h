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
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fuse/Dispatcher.h"
#include "eden/fuse/fuse_headers.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class EdenMount;
class TreeInode;

class InodeBase : public std::enable_shared_from_this<InodeBase> {
 public:
  /**
   * Constructor for the root TreeInode of an EdenMount.
   */
  explicit InodeBase(EdenMount* mount);

  /**
   * Constructor for all non-root inodes.
   */
  InodeBase(fuse_ino_t ino, TreeInodePtr parent, PathComponentPiece name);

  virtual ~InodeBase();

  fuse_ino_t getNodeId() const {
    return ino_;
  }

  /**
   * Increment the number of outstanding FUSE references to an inode number.
   *
   * This should be called in response to a FUSE lookup() call.
   */
  void incNumFuseLookups() {
    numFuseReferences_.fetch_add(1, std::memory_order_acq_rel);
  }

  /**
   * Decrement the number of outstanding FUSE references to an inode number.
   *
   * This should be called in response to a FUSE forget() call.
   */
  void decNumFuseLookups() {
    numFuseReferences_.fetch_sub(1, std::memory_order_acq_rel);
  }

  /**
   * Get the EdenMount that this inode belongs to
   *
   * The EdenMount is guaranteed to remain valid for at least the lifetime of
   * this InodeBase object.
   */
  EdenMount* getMount() const {
    return mount_;
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
   * Short term hack for existing code that is incorrectly using the path in
   * racy ways.
   *
   * This is mostly used by the Overlay code.  The Overlay code needs to be
   * switched to use inode numbers instead of path names.
   *
   * The value returned is not guaranteed to be up-to-date by the time it is
   * used.  This may also throw if the file has been unlinked.
   *
   * TODO: Remove this method once the Overlay code is updated to use inode
   * numbers instead of path names.
   */
  RelativePath getPathBuggy() const {
    return getPath().value();
  }

  /**
   * Get a string to use to refer to this file in a log message.
   *
   * This will usually return the path to the file, but if the file has been
   * unlinked it will return a string with data about where the file used to
   * exist.  The result is human-readable and is not designed for consumption
   * or parsing by other code.
   */
  std::string getLogPath() const;

  /**
   * markUnlinked() should only be invoked by TreeInode.
   *
   * This method is called when a child inode is unlinked from its parent.
   * This can happen in a few different ways:
   *
   * - By TreeInode::unlink() (for FileInode objects)
   * - By TreeInode::rmdir() (for TreeInode objects)
   * - By TreeInode::rename() for the destination of the rename,
   *   (which may be a file or an empty tree inode)
   *
   * TODO: Once we have a rename lock, this method should take a const
   * reference to the mountpoint-wide rename lock to guarantee the caller is
   * properly holding the lock.
   */
  void markUnlinked();

  /**
   * updateLocation() should only be invoked by TreeInode.
   *
   * This is called when an inode is renamed to a new location.
   *
   * TODO: Once we have a rename lock, this method should take a const
   * reference to the mountpoint-wide rename lock to guarantee the caller is
   * properly holding the lock.
   */
  void updateLocation(TreeInodePtr newParent, PathComponentPiece newName);

 protected:
  /**
   * TODO: A temporary hack for children inodes looking up their parent without
   * proper locking.
   *
   * At the moment this is primarily use for dealing with the Overlay.  The
   * right long-term fix is to just change the overlay so that the path to an
   * inodes overlay data depends only on its inode number, and not on is path.
   * As-is, the overlay code is racy with respect to rename() and unlink()
   * operations.
   *
   * TODO: Remove this method once the Overlay code is updated to use inode
   * numbers instead of path names.
   */
  TreeInodePtr getParentBuggy() {
    return location_.rlock()->parent;
  }

 private:
  struct LocationInfo {
    LocationInfo(TreeInodePtr p, PathComponentPiece n)
        : parent(std::move(p)), name(n) {}

    TreeInodePtr parent;
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

  /**
   * The EdenMount object that this inode belongs to.
   *
   * We store this as a raw pointer since the TreeInode is part of the mount
   * point.  The EdenMount will always exist longer than any inodes it
   * contains.
   */
  EdenMount* const mount_{nullptr};

  /**
   * A reference count tracking the outstanding lookups that the kernel's FUSE
   * API has performed on this inode.  We must remember this inode number
   * for as long as the FUSE API has references to it.  (However, we may unload
   * the Inode object itself, destroying ourself and letting the InodeMap
   * simply remember the association of the fuse_ino_t with our location in the
   * file system.)
   */
  std::atomic<uint32_t> numFuseReferences_{0};

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
