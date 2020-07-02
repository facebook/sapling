/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"

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
  class TreePrefetchContext : public ObjectFetchContext {
   public:
    ImportPriority getPriority() const override {
      return ImportPriority::kLow();
    }
  };

 public:
  explicit TreePrefetchLease(TreeInodePtr inode)
      : inode_{std::move(inode)},
        context_(std::make_unique<TreePrefetchContext>()) {}

  ~TreePrefetchLease() {
    release();
  }
  TreePrefetchLease(TreePrefetchLease&& lease) noexcept
      : inode_{std::move(lease.inode_)}, context_(std::move(lease.context_)) {}
  TreePrefetchLease& operator=(TreePrefetchLease&& lease) noexcept {
    if (&lease != this) {
      release();
      inode_ = std::move(lease.inode_);
      context_ = std::move(lease.context_);
    }
    return *this;
  }

  const TreeInodePtr& getTreeInode() const {
    return inode_;
  }

  ObjectFetchContext& getContext() const {
    return *context_;
  }

 private:
  TreePrefetchLease(const TreePrefetchLease& lease) = delete;
  TreePrefetchLease& operator=(const TreePrefetchLease& lease) = delete;

  void release() noexcept;

  TreeInodePtr inode_;

  std::unique_ptr<ObjectFetchContext> context_;
};

} // namespace eden
} // namespace facebook
