/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathMap.h"

namespace facebook {
namespace eden {

/**
 * Represents a directory entry.
 *
 * A directory entry can be in one of several states:
 *
 * - An InodeBase object for the entry may or may not exist.  If it does
 *   exist, it is the authoritative source of data for the entry.
 *
 * - If the child InodeBase object does not exist, we may or may not have an
 *   inode number already allocated for the child.  An inode number can be
 *   allocated on-demand if necessary, without fully creating a child
 *   InodeBase object.
 *
 * - The child may or may not be materialized in the overlay.
 *
 *   If the child contents are identical to an existing source control Tree
 *   or Blob then it does not need to be materialized, and the Entry may only
 *   contain the hash identifying the Tree/Blob.
 *
 *   If the child is materialized in the overlay, then it must have an inode
 *   number allocated to it.
 */
class DirEntry {
 public:
  /**
   * Create a hash for a non-materialized entry.
   */
  DirEntry(mode_t m, InodeNumber number, Hash hash)
      : mode_{m}, hasHash_{true}, hash_{hash}, inodeNumber_{number} {
    DCHECK(number.hasValue());
  }

  /**
   * Create a hash for a materialized entry.
   */
  DirEntry(mode_t m, InodeNumber number) : mode_{m}, inodeNumber_{number} {
    DCHECK(number.hasValue());
  }

  DirEntry(DirEntry&& e) = default;
  DirEntry& operator=(DirEntry&& e) = default;
  DirEntry(const DirEntry& e) = delete;
  DirEntry& operator=(const DirEntry& e) = delete;

  bool isMaterialized() const {
    // TODO: In the future we should probably only allow callers to invoke
    // this method when inode is not set.  If inode is set it should be the
    // authoritative source of data.
    return !hasHash_;
  }

  Hash getHash() const {
    // TODO: In the future we should probably only allow callers to invoke
    // this method when inode is not set.  If inode is set it should be the
    // authoritative source of data.
    DCHECK(hasHash_);
    return hash_;
  }

  folly::Optional<Hash> getOptionalHash() const {
    if (hasHash_) {
      return hash_;
    } else {
      return folly::none;
    }
  }

  InodeNumber getInodeNumber() const;

  void setMaterialized() {
    hasHash_ = false;
  }

  void setDematerialized(Hash hash) {
    DCHECK(hasInodePointer_);
    hasHash_ = true;
    hash_ = hash;
  }

  mode_t getMode() const {
    // Callers should not check getMode() if an inode is loaded.
    // If the child inode is loaded it is the authoritative source for
    // the mode bits.
    DCHECK(!hasInodePointer_);
    return mode_;
  }

  mode_t getModeUnsafe() const {
    // TODO: T20354866 Remove this method once all callers are refactored.
    //
    // Callers should always call getMode() instead. This method only exists
    // for supporting legacy code which will be refactored eventually.
    return mode_;
  }

  /**
   * Get the file type, as a dtype_t value as used by readdir()
   *
   * It is okay for callers to call getDtype() even if the inode is
   * loaded.  The file type for an existing entry never changes.
   */
  dtype_t getDtype() const {
    return mode_to_dtype(mode_);
  }

  /**
   * Check if the entry is a directory or not.
   *
   * It is okay for callers to call isDirectory() even if the inode is
   * loaded.  The file type for an existing entry never changes.
   */
  bool isDirectory() const {
    return dtype_t::Dir == getDtype();
  }

  InodeBase* getInode() const {
    return hasInodePointer_ ? inode_ : nullptr;
  }

  InodePtr getInodePtr() const {
    // It's safe to call newPtrLocked because calling getInode() implies the
    // TreeInode's contents_ lock is held.
    return hasInodePointer_ ? InodePtr::newPtrLocked(inode_) : InodePtr{};
  }

  /**
   * Same as getInodePtr().asFilePtrOrNull() except it avoids constructing
   * a FileInodePtr if the entry does not point to a FileInode.
   */
  FileInodePtr asFilePtrOrNull() const;

  /**
   * Same as getInodePtr().asTreePtrOrNull() except it avoids constructing
   * a TreeInodePtr if the entry does not point to a FileInode.
   */
  TreeInodePtr asTreePtrOrNull() const;

  void setInode(InodeBase* inode);

  void clearInode();

 private:
  /**
   * The initial entry type for this entry.
   */
  mode_t mode_{0};

  // Can we borrow some bits from mode_t? :) If so, Entry would fit in 4
  // words.
  bool hasHash_{false};
  bool hasInodePointer_{false};

  /**
   * If the entry is not materialized, this contains the hash
   * identifying the source control Tree (if this is a directory) or Blob
   * (if this is a file) that contains the entry contents.
   *
   * If the entry is materialized, hasHash_ is false.
   *
   * TODO: If inode is set, this field generally should not be used, and the
   * child InodeBase should be consulted instead.
   */
  Hash hash_;

  union {
    /**
     * The inode number, if one is allocated for this entry, or 0 if one is
     * not allocated.
     *
     * An inode number is required for materialized entries, so this is always
     * non-zero if hash_ is not set.  (It may also be non-zero even when hash_
     * is set.)
     */
    InodeNumber inodeNumber_{0};

    /**
     * A pointer to the child inode, if it is loaded, or null if it is not
     * loaded.
     *
     * Note that we store this as a raw pointer.  Children inodes hold a
     * reference to their parent TreeInode, not the other way around.
     * Children inodes can be destroyed only in one of two ways:
     * - Being unlinked, then having their last reference go away.
     *   In this case they will be removed from our entries list when they are
     *   unlinked.
     * - Being unloaded (after their reference count is already 0).  In this
     *   case the parent TreeInodes responsible for triggering unloading of
     *   its children, so it resets this pointer to null when it unloads the
     *   child.
     */
    InodeBase* inode_;
  };
};

// TODO: We can still do better than this. When the Entry only holds dtype_t
// instead of mode_t, this will fit in 32 bytes, which would be a material
// savings given how many trees Eden tends to keep loaded.
static_assert(sizeof(DirEntry) == 40, "DirEntry is five words");

/**
 * Represents a directory in the overlay.
 */
struct DirContents {
  /** The direct children of this directory */
  PathMap<DirEntry> entries;
};

} // namespace eden
} // namespace facebook
