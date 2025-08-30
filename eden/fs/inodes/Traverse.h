/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <variant>
#include "eden/common/utils/DirType.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {

class DirEntry;
class TreeInode;

/**
 * Represents a TreeInode entry. Populated from the interesting fields of
 * DirEntry and can be used while the TreeInode contents lock is not held.
 */
struct ChildEntry {
  PathComponent name;
  dtype_t dtype;
  InodeNumber ino;
  std::optional<ObjectId> id;

  // Null if the entry has no loaded inode.
  InodePtr loadedChild;
};

struct TraversalCallbacks {
  virtual ~TraversalCallbacks() {}

  /**
   * Called for every allocated TreeInode, whether loaded or not.
   */
  virtual void visitTreeInode(
      RelativePathPiece path,
      InodeNumber ino,
      const std::optional<ObjectId>& id,
      uint64_t fsRefcount,
      const std::vector<ChildEntry>& entries) = 0;

  /**
   * Called for every ChildEntry of a TreeInode. Returns whether traversal
   * should recurse to the entry's children.
   */
  virtual bool shouldRecurse(const ChildEntry& entry) = 0;
};

/**
 * Starting from the given loaded TreeInode root, performs a pre-order traversal
 * of EdenFS's observed inode tree structure.
 *
 * This function will never load new Tree objects from the backing store, never
 * allocate new inodes in the overlay, nor load previously-allocated inodes into
 * memory. It will, however, traverse previously-allocated inodes from the
 * Overlay.
 *
 * Thus, this function can give a complete view into checkout as far as EdenFS
 * has observed to this point.
 */
void traverseObservedInodes(
    const TreeInode& root,
    RelativePathPiece rootPath,
    TraversalCallbacks& callbacks);

} // namespace facebook::eden
