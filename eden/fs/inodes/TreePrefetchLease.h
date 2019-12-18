/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/TreeInode.h"

namespace facebook {
namespace eden {

/**
 * TreePrefetchLease is a small helper class to track the total number of
 * concurrent tree prefetch operations running in an EdenMount.
 *
 * When TreeInode wants to perform a prefetch it should call
 * EdenMount::tryStartTreePrefetch() to obtain a prefetch lease.  If it obtains
 * a lease it can perform the prefetch, and should hold the TreePrefetchLease
 * object around until the prefetch completes.  When the TreePrefetchLease is
 * destroyed this will inform the EdenMount that the prefetch is complete.
 */
class TreePrefetchLease {
 public:
  explicit TreePrefetchLease(TreeInodePtr inode) : inode_{std::move(inode)} {}
  ~TreePrefetchLease() {
    release();
  }
  TreePrefetchLease(TreePrefetchLease&& lease) noexcept
      : inode_{std::move(lease.inode_)} {}
  TreePrefetchLease& operator=(TreePrefetchLease&& lease) noexcept {
    if (&lease != this) {
      release();
      inode_ = std::move(lease.inode_);
    }
    return *this;
  }

  const TreeInodePtr& getTreeInode() const {
    return inode_;
  }

 private:
  TreePrefetchLease(const TreePrefetchLease& lease) = delete;
  TreePrefetchLease& operator=(const TreePrefetchLease& lease) = delete;

  void release() noexcept;

  TreeInodePtr inode_;
};

} // namespace eden
} // namespace facebook
