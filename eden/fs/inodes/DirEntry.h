/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <optional>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathMap.h"
#include "eden/fs/utils/StaticAssert.h"

namespace facebook::eden {

/**
 * Represents a directory entry.
 *
 * A directory entry has two independent state conditions:
 *
 * - An InodeBase object for the entry may or may not exist. If it does
 *   exist, it is the authoritative source of data for the entry. If not, the
 *   type of the entry can be retrieved, but to read or update its contents or
 *   inode metadata, the InodeBase must be loaded.
 *
 * - The child may or may not be materialized in the overlay.
 *   If the child contents are identical to an existing source control Tree
 *   or Blob then it does not need to be materialized, and the Entry may only
 *   contain the hash identifying the Tree/Blob. If the entry is materialized,
 *   no hash is set and the entry's materialized contents are available in the
 *   Overlay under the entry's inode number.
 */
class DirEntry {
 public:
  /**
   * Create a hash for a non-materialized entry.
   */
  DirEntry(mode_t m, InodeNumber number, ObjectId hash)
      : initialMode_{m},
        hasHash_{true},
        hasInodePointer_{false},
        hash_{hash},
        inodeNumber_{number} {
    XCHECK_EQ(m, m & 0x3fffffff);
    XDCHECK(number.hasValue());
  }

  /**
   * Create a hash for a materialized entry.
   */
  DirEntry(mode_t m, InodeNumber number)
      : initialMode_{m},
        hasHash_{false},
        hasInodePointer_{false},
        inodeNumber_{number} {
    XCHECK_EQ(m, m & 0x3fffffff);
    XDCHECK(number.hasValue());
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

  ObjectId getHash() const {
    // TODO: In the future we should probably only allow callers to invoke
    // this method when inode is not set.  If inode is set it should be the
    // authoritative source of data.
    XDCHECK(hasHash_);
    return hash_;
  }

  std::optional<ObjectId> getOptionalHash() const {
    if (hasHash_) {
      return hash_;
    } else {
      return std::nullopt;
    }
  }

  InodeNumber getInodeNumber() const;

  void setMaterialized() {
    hasHash_ = false;
  }

  void setDematerialized(ObjectId hash) {
    XDCHECK(hasInodePointer_);
    hasHash_ = true;
    hash_ = hash;
  }

  /**
   * Returns the mode specified when this inode was created (whether from source
   * control or via mkdir/mknod/creat).
   *
   * Note: when the mode_t for an inode changes, this value does not update.
   */
  mode_t getInitialMode() const {
    return static_cast<mode_t>(initialMode_);
  }

  /**
   * Get the file type, as a dtype_t value as used by readdir()
   *
   * It is okay for callers to call getDtype() even if the inode is
   * loaded.  The file type for an existing entry never changes.
   */
  dtype_t getDtype() const {
    return mode_to_dtype(getInitialMode());
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

  /**
   * Similar to asTreePtrOrNull() except it returns TreeInode* to avoid
   * dereferring TreeInodePtr that could potentially deadlock.
   */
  TreeInode* FOLLY_NULLABLE asTreeOrNull() const;

  /**
   * Associates a loaded inode pointer with this entry. Does not take ownership.
   */
  void setInode(InodeBase* inode);

  /**
   * Clears and returns this entry's inode pointer. Must only be called if
   * assignInode() has.
   *
   * This method is only called when the inode is being unloaded and its pointer
   * is no longer valid.
   */
  FOLLY_NODISCARD InodeBase* clearInode();

 private:
  /**
   * The initial entry type for this entry. Two bits are borrowed from the top
   * so the entire struct fits in four words.
   *
   * TODO: This field is not updated when an inode's mode bits are changed.
   * For now, it's used primarily to migrate data without loss from the old
   * Overlay Dir storage. After the InodeMetadataTable is in use for a while,
   * this should be replaced with dtype_t and the bitfields can go away.
   */
  uint32_t initialMode_ : 30;

  /**
   * Whether the hash_ field matches the contents from source control. If
   * hasHash_ is false, the entry is materialized.
   */
  uint32_t hasHash_ : 1;

  /**
   * If true, the inode_ field is valid. If false, inodeNumber_ is valid.
   * Synonymous with the inode being "loaded".
   */
  uint32_t hasInodePointer_ : 1;

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
  ObjectId hash_;

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

static_assert(CheckSize<DirEntry, 40>(), "DirEntry is five words");

/**
 * Represents a directory in the overlay.
 */
struct DirContents : PathMap<DirEntry> {
  explicit DirContents(CaseSensitivity caseSensitive)
      : PathMap(caseSensitive) {}
};

} // namespace facebook::eden
