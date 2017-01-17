/*
 *  Copyright (c) 2017, Facebook, Inc.
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

class InodeBase {
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
  template <typename InodeType>
  friend class InodePtrImpl;
  friend class InodePtrTestHelper;

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

  // incrementPtrRef() is called by InodePtr whenever an InodePtr is copied.
  void incrementPtrRef() const {
    auto prevValue = ptrRefcount_.fetch_add(1, std::memory_order_acq_rel);
    // Calls to incrementPtrRef() are not allowed to increment the reference
    // count from 0 to 1.
    //
    // The refcount is only allowed to go from 0 to 1 when holding the InodeMap
    // lock or our parent TreeInode's contents lock.  Those two situations call
    // newInodeRefConstructed() instead
    DCHECK_NE(0, prevValue);
  }

  // newInodeRefConstructed() is called any time we construct a brand new
  // InodePtr in response to a request to access or load an Inode.  The only
  // APIs that hand out new InodePtrs are InodeMap::lookupInode() and
  // TreeInode::getOrLoadChild().
  void newInodeRefConstructed() const {
    ptrRefcount_.fetch_add(1, std::memory_order_acq_rel);
  }
  void decrementPtrRef() const {
    ptrRefcount_.fetch_sub(1, std::memory_order_acq_rel);
  }

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
   * A reference count used by InodePtr.
   *
   * A few notes about the refcount management:
   *
   * - Inode objects are not necessarily destroyed immediately when the
   *   refcount goes to 0.  They may remain in memory for a while in case they
   *   get used again relatively soon.  When necessary we can sweep the loaded
   *   inode objects and unload inodes whose refcount is 0 and who have not
   *   been accessed recently.
   *
   * - When copying or deleting InodePtr objects this reference count is
   *   updated atomically with acquire/release barriers.  No other locks need
   *   to be held during these operations.  The current thread is guaranteed to
   *   already hold a reference to the Inode in question since it already has
   *   an InodePtr.  These operations can increment a refcount from 1 or more
   *   to a higher value, but they can never increment a refcount from 0 to 1.
   *   They can also decrement a refcount from 1 to 0.
   *
   * - Either the InodeMap lock or the parent TreeInode's contents lock is
   *   always held when incrementing the refcount from 0 to 1.
   *
   *   Only two operations can inrement the refcount from 0 to 1:
   *   - InodeMap::lookupInode().
   *     This acquires the InodeMap lock
   *   - TreeInode::getOrLoadChild()
   *     This acquires the parent's TreeInode lock
   *
   *   When checking to see if we can unload an inode, we acquire both it's
   *   parent TreeInode's contents lock and the InodeMap lock (in that order).
   *   We are therefore guaranteed that if the refcount is 0 when we check it,
   *   no other thread can increment it to 1 before we delete the object.
   *
   * Notes about owning vs non-owning pointers:
   * - An Inode always holds an owning TreeInodePtr to its parent.  This
   *   ensures the parent cannot be unloaded as long as it has any unloaded
   *   children.
   *
   * - The InodeMap stores raw (non-owning) pointers to the inodes.  When an
   *   Inode is unloaded we explicitly inform the InodeMap of the change.
   *
   * - Each TreeInode holds raw (non-owning) pointers to its children.  When an
   *   Inode is unloaded we explicitly reset its parent pointer to this object.
   *
   * - The numFuseReferences_ variable tracks the number of users that know
   *   about this inode by its inode number.  However, this does not prevent us
   *   from destroying the Inode object.  We can unload the Inode object itself
   *   in this case, and InodeMap will retain enough information to be able to
   *   re-create the Inode object later if this inode is looked up again.
   */
  mutable std::atomic<uint32_t> ptrRefcount_{0};

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

#include "eden/fs/inodes/InodePtr-defs.h"
