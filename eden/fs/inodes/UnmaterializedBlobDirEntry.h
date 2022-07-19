/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/utils/DirType.h"

namespace facebook::eden {

/**
 * A fake directory entry for use inside VirtualInode.
 *
 * The VirtualInode class allows callers to see a "mixed" view of the eden
 * mount, which respresents both on-disk (inode) state, and in-backing-store
 * (source control) state. When a DirEntry represents a ObjectStore object that
 * doesn't exist on disk (isn't loaded, isn't materialized), some of the
 * contents of DirEntry must be returned to represent the object (particularly
 * the ObjectId), but DirEntry can't be safely copied, as it protected by the
 * holding-Inode's contents() lock.
 *
 * This class copies enough of the DirEntry to be able to reason about the
 * underlying objects, and is safe to copy around.
 */
class UnmaterializedBlobDirEntry {
 public:
  // Note that these objects are only constructed when it is known that the
  // entry.getHash() exists. See TreeInode::getOrFindChild()
  explicit UnmaterializedBlobDirEntry(const DirEntry& entry)
      : hash_(entry.getHash()), dtype_(entry.getDtype()) {}

  const ObjectId getHash() const {
    return hash_;
  }

  dtype_t getDtype() const {
    return dtype_;
  }

  bool isDirectory() const {
    return getDtype() == dtype_t::Dir;
  }

 private:
  ObjectId hash_;
  dtype_t dtype_;
};

} // namespace facebook::eden
