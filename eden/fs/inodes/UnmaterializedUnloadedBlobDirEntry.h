/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/utils/DirType.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {

/**
 * A fake directory entry for use inside VirtualInode.
 *
 * The VirtualInode class allows callers to see a "mixed" view of the eden
 * mount, which represents both on-disk (inode) state, and in-backing-store
 * (source control) state. When a DirEntry represents a ObjectStore object that
 * doesn't exist on disk (isn't loaded, isn't materialized), some of the
 * contents of DirEntry must be returned to represent the object (particularly
 * the ObjectId), but DirEntry can't be safely copied, as it protected by the
 * holding-Inode's contents() lock.
 *
 * This class copies enough of the DirEntry to be able to reason about the
 * underlying objects, and is safe to copy around.
 */
class UnmaterializedUnloadedBlobDirEntry {
 public:
  // Note that these objects are only constructed when it is known that the
  // entry.getObjectId() exists. See TreeInode::getOrFindChild()
  explicit UnmaterializedUnloadedBlobDirEntry(const DirEntry& entry)
      : objectId_(entry.getObjectId()),
        dtype_(entry.getDtype()),
        initialMode_(entry.getInitialMode()) {}

  UnmaterializedUnloadedBlobDirEntry(
      const UnmaterializedUnloadedBlobDirEntry&) = default;
  UnmaterializedUnloadedBlobDirEntry(UnmaterializedUnloadedBlobDirEntry&&) =
      default;

  UnmaterializedUnloadedBlobDirEntry& operator=(
      const UnmaterializedUnloadedBlobDirEntry&) = default;
  UnmaterializedUnloadedBlobDirEntry& operator=(
      UnmaterializedUnloadedBlobDirEntry&&) = default;

  const ObjectId& getObjectId() const {
    return objectId_;
  }

  dtype_t getDtype() const {
    return dtype_;
  }

  /**
   * The initial mode of the shadowed DirEntry.
   *
   * Note that these objects are only created for unloaded/unmaterialized
   * inodes, so the initialMode is a good representation of the mode just after
   * loading.
   */
  mode_t getInitialMode() const {
    return initialMode_;
  }

 private:
  ObjectId objectId_;
  dtype_t dtype_;
  mode_t initialMode_;
};

} // namespace facebook::eden
