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
class ParentInodeInfo;
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
   * Increment the number of references to this inode by its inode number.
   *
   * While the FUSE reference count is non-zero, the inode number will be
   * remembered, and InodeMap::lookupInode() can be used to look up the inode
   * object using its inode number.  Once the FUSE reference count drops to
   * zero the inode number may be forgotten, and it is no longer valid to call
   * InodeMap::lookupInode() with this inode's number.
   *
   * This is generally intended for use by FUSE APIs that return an inode
   * number to the kernel: lookup(), create(), mkdir(), symlink(), link()
   */
  void incFuseRefcount() {
    numFuseReferences_.fetch_add(1, std::memory_order_acq_rel);
  }

  /**
   * Decrement the number of outstanding references to this inode's number.
   *
   * This should be used to release inode number references obtained via
   * incFuseRefcount().  The primary use case is for FUSE forget() calls.
   */
  void decFuseRefcount(uint32_t count = 1) {
    auto prevValue =
        numFuseReferences_.fetch_sub(count, std::memory_order_acq_rel);
    DCHECK_GE(prevValue, count);
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
   * Check if this Inode has been unlinked from its parent TreeInode.
   *
   * Once an inode is unlinked it is no longer part of the file system tree.
   * It can still be accessed by existing FileHandles or other internal
   * InodePtrs referring to it, but it can no longer be accessed by a path.
   *
   * An unlinked Inode can never be re-linked back into the file system.
   * It will be destroyed when the last reference to it goes away.
   *
   * TreeInodes can only be unlinked when they have no children.  It is
   * therefore not possible to have an Inode object that is not marked unlinked
   * but has a parent tree that is unlinked.
   */
  bool isUnlinked() const;

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
   * This must be called while holding the parent's contents_ lock.
   *
   * TODO: Once we have a rename lock, this method should take a const
   * reference to the mountpoint-wide rename lock to guarantee the caller is
   * properly holding the lock.
   *
   * Unlinking an inode may cause it to be immediately unloaded.  If this
   * occurs, this method returns a unique_ptr to itself.  The calling TreeInode
   * is then responsible for actually deleting the inode (which will happen
   * automatically when the unique_ptr is destroyed or reset) in their calling
   * context after they release their contents lock.  If unlinking this inode
   * does not cause it to be immediately unloaded then this method will return
   * a null pointer.
   */
  std::unique_ptr<InodeBase> markUnlinked(
      TreeInode* parent,
      PathComponentPiece name);

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

  /**
   * Check to see if the ptrAcquire reference count is zero.
   *
   * This method is intended for internal use by the InodeMap/TreeInode code,
   * so it can tell when it is safe to unload an inode.
   *
   * This method should only be called while holding both the parent
   * TreeInode's contents lock and the InodeMap lock.  (Otherwise the reference
   * count may be incremented by another thread before you can examine the
   * return value.)
   */
  bool isPtrAcquireCountZero() const {
    return ptrAcquireCount_.load(std::memory_order_acquire) == 0;
  }

  /**
   * Decrement the ptrAcquire reference count, and return its previous value.
   *
   * This method is intended for internal use by the InodeMap/TreeInode code,
   * so it can tell when it is safe to unload an inode.
   *
   * This method should only be called while holding both the parent
   * TreeInode's contents lock and the InodeMap lock.  (Otherwise the reference
   * count may be incremented by another thread before you can examine the
   * return value.)
   */
  uint32_t decPtrAcquireCount() const {
    return ptrAcquireCount_.fetch_sub(1, std::memory_order_acq_rel);
  }

  /**
   * Get the FUSE reference count.
   *
   * This is intended only to be checked when an Inode is being unloaded,
   * while holding both it's parent TreeInode's contents_ lock and the InodeMap
   * lock.
   *
   * The FUSE reference count is only incremented or decremented while holding
   * a pointer reference on the Inode.  Checking the FUSE reference count is
   * therefore safe during unload, when we are sure there are no outstanding
   * pointer references to the inode.
   *
   * Checking the FUSE reference count at any other point in time may be racy,
   * since other threads may be changing the reference count concurrently.
   */
  uint32_t getFuseRefcount() const {
    // DCHECK that the caller is only calling us while the inode is being
    // unloaded
    DCHECK_EQ(0, ptrAcquireCount_.load(std::memory_order_acquire));

    return numFuseReferences_.load(std::memory_order_acquire);
  }

  /**
   * Set the FUSE reference count.
   *
   * This method should only be called by InodeMap when first loading an Inode,
   * before the Inode object has been returned to any users.
   */
  void setFuseRefcount(uint32_t count) {
    return numFuseReferences_.store(count, std::memory_order_release);
  }

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
    auto prevValue = ptrRefcount_.fetch_add(1, std::memory_order_acq_rel);
    if (prevValue == 0) {
      ptrAcquireCount_.fetch_add(1, std::memory_order_acq_rel);
    }
  }
  void decrementPtrRef() const {
    auto prevValue = ptrRefcount_.fetch_sub(1, std::memory_order_acq_rel);
    if (prevValue == 1) {
      onPtrRefZero();
    }
  }
  void onPtrRefZero() const;
  ParentInodeInfo getParentInfo() const;

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
   * The number of times the ptrRefcount_ has been incremented from 0 to 1,
   * minus the number of times it has been decremented from 1 to 0.
   *
   * This is necessary so we can properly synchronize destruction, and ensure
   * that only one thread tries to destroy a given Inode.
   *
   * This variable can only be incremented when holding either the parent
   * TreeInode's contents_ lock or the InodeMap lock.  It can only be
   * decremented when holding both the parent TreeInode's contents_ lock and
   * the InodeMap lock.  When ptrAcquireCount_ drops to 0 it is safe to delete
   * the Inode.
   *
   * It isn't safe to delete the Inode purely based on ptrRefcount_ alone,
   * since ptrRefcount_ is decremented without holding any other locks.  It's
   * possible that thread A ptrRefcount_ drops to 0 and then thread B
   * immediately increments ptrRefcount_ back to 1.  If thread B then drops the
   * refcount back to 0 we need to make sure that only one of thread A and
   * thread B try to destroy the inode.
   *
   * By tracking ptrRefcount_ and ptrAcquireCount_ separately we allow
   * ptrRefcount_ to be manipulated with a single atomic operation in most
   * cases (when not transitioning between 0 and 1).  Only when transitioning
   * from 0 to 1 or vice-versa do we need to acquire additional locks and
   * perform more synchronization.
   */
  mutable std::atomic<uint32_t> ptrAcquireCount_{0};

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
