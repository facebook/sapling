/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/InodePtr.h"

namespace facebook::eden {

class EdenMount;
class Journal;
class ObjectStore;

/**
 * Operations on mounts need to ensure the EdenMount is not deleted
 * for the duration.  EdenMountHandle holds a reference to the mount
 * and its root inode and ensures the mount is usable while the
 * EdenMountHandle lives.
 */
class EdenMountHandle {
 public:
  EdenMountHandle(std::shared_ptr<EdenMount> edenMount, TreeInodePtr rootInode)
      : edenMount_{std::move(edenMount)}, rootInode_{std::move(rootInode)} {}

  EdenMountHandle(const EdenMountHandle&) = default;
  EdenMountHandle(EdenMountHandle&&) = default;
  EdenMountHandle& operator=(const EdenMountHandle&) = default;
  EdenMountHandle& operator=(EdenMountHandle&&) = default;

  /**
   * Returns a reference to EdenMount to indicate the reference is unowned. To
   * ensure that an EdenMount can be used, the EdenMountHandle must be held.
   */
  EdenMount& getEdenMount() const {
    return *edenMount_;
  }

  // TODO: Remove, preferring getEdenMount()
  const std::shared_ptr<EdenMount>& getEdenMountPtr() const {
    return edenMount_;
  }

  const TreeInodePtr& getRootInode() const {
    return rootInode_;
  }

  // Convenience methods that support common uses of lookupMount().

  ObjectStore& getObjectStore() const;
  const std::shared_ptr<ObjectStore>& getObjectStorePtr() const;
  Journal& getJournal() const;

 private:
  std::shared_ptr<EdenMount> edenMount_;
  // Today, holding a reference to the root inode is what keeps the mount alive
  // and usable.
  TreeInodePtr rootInode_;
};

} // namespace facebook::eden
