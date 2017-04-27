/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Synchronized.h>
#include <vector>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/model/ParentCommits.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class exception_wrapper;
template <typename T>
class Future;
class Unit;
}

namespace facebook {
namespace eden {

class CheckoutConflict;
class TreeInode;
class Tree;

/**
 * CheckoutContext maintains state during a checkout operation.
 */
class CheckoutContext {
 public:
  CheckoutContext(
      folly::Synchronized<ParentCommits>::LockedPtr&& parentsLock,
      bool force);
  ~CheckoutContext();

  /**
   * Returns true if the checkout operation should actually update the inodes,
   * or false if it should do a dry run, looking for conflicts without actually
   * updating the inode contents.
   */
  bool shouldApplyChanges() const {
    // TODO: make this configurable on checkout start
    return true;
  }

  /**
   * Returns true if this checkout operation should force the new inode
   * contents to look like the data in the Tree being checked out, even if
   * there are conflicts.
   *
   * This will cause the checkout to always update files with conflicts to the
   * new contents, rather than just reporting and skipping files with
   * conflicts.
   *
   * forceUpdate() can only return true when shouldApplyChanges() is also true.
   */
  bool forceUpdate() const {
    return force_;
  }

  /**
   * Start the checkout operation.
   */
  void start(RenameLock&& renameLock);

  /**
   * Complete the checkout operation
   *
   * Returns the list of conflicts and errors that were encountered during the
   * operation.
   */
  std::vector<CheckoutConflict> finish(Hash newSnapshot);

  void addConflict(ConflictType type, RelativePathPiece path);
  void
  addConflict(ConflictType type, TreeInode* parent, PathComponentPiece name);
  void addConflict(ConflictType type, InodeBase* inode);

  void addError(
      TreeInode* parent,
      PathComponentPiece name,
      const folly::exception_wrapper& ew);

  /**
   * Get a reference to the rename lock.
   *
   * This is mostly used for APIs that require proof that we are currently
   * holding the lock.
   */
  const RenameLock& renameLock() const {
    return renameLock_;
  }

 private:
  bool const force_{false};
  folly::Synchronized<ParentCommits>::LockedPtr parentsLock_;
  RenameLock renameLock_;

  // The checkout processing may occur across many threads,
  // if some data load operations complete asynchronously on other threads.
  // Therefore access to the conflicts list must be synchronized.
  folly::Synchronized<std::vector<CheckoutConflict>> conflicts_;
};
}
}
