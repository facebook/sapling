/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Synchronized.h>
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * ParentInodeInfo contains information about an InodeBase's parent.
 *
 * This object hold the lock on the parent TreeInode's contents for as long as
 * it exists.  This ensures that the Inode in question cannot be renamed or
 * unlinked while the ParentInodeInfo object exists.
 *
 * Note that we intentionally hold the parent TreeInode's contents lock, and
 * not this Inode's location_ lock.  The location_ lock would also prevent
 * changes to the location, but this lock is very low-level in our lock
 * ordering scheme, and no other locks may be held while holding it.  This
 * prevents us from doing many useful operations.  Additionally, most
 * operations where we need to use the ParentInodeInfo requires us to hold the
 * parent's lock anyway.
 */
class ParentInodeInfo {
 public:
  ParentInodeInfo(
      PathComponentPiece name,
      TreeInodePtr parent,
      bool isUnlinked,
      folly::Synchronized<TreeInodeState>::LockedPtr contents)
      : name_(name),
        parent_(std::move(parent)),
        isUnlinked_(isUnlinked),
        parentContents_(std::move(contents)) {}

  /**
   * Get a pointer to the parent.
   *
   * This will return a null pointer if this the root inode.
   * In all other cases this will return non-null, including for unlinked
   * inodes.
   *
   * For unlinked inodes this returns a pointer to the inode that used to be
   * the parent just before this inode was unlinked.  Note that in this case
   * the parent itself may also be unlinked.
   */
  const TreeInodePtr& getParent() const {
    return parent_;
  }

  /**
   * Returns true if this inode has been unlinked from its parent.
   */
  bool isUnlinked() const {
    return isUnlinked_;
  }

  /**
   * Get the name of this inode inside its parent.
   *
   * For unlinked inodes this returns its name just before it was unlinked.
   */
  const PathComponent& getName() const {
    return name_;
  }

  /**
   * Get the locked contents of the parent inode.
   *
   * This returns a null pointer if this is the root inode, or if this inode is
   * unlinked.
   */
  const folly::Synchronized<TreeInodeState>::LockedPtr& getParentContents()
      const {
    return parentContents_;
  }

  void reset() {
    if (parentContents_) {
      parentContents_.unlock();
    }
  }

 private:
  PathComponent name_;
  TreeInodePtr parent_;
  bool isUnlinked_;
  folly::Synchronized<TreeInodeState>::LockedPtr parentContents_;
};
} // namespace eden
} // namespace facebook
