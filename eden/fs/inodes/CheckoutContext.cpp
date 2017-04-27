/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/CheckoutContext.h"

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/TreeInode.h"

using folly::Future;
using folly::Unit;
using std::vector;

namespace facebook {
namespace eden {

CheckoutContext::CheckoutContext(
    folly::Synchronized<ParentCommits>::LockedPtr&& parentsLock,
    bool force)
    : force_{force}, parentsLock_(std::move(parentsLock)) {}

CheckoutContext::~CheckoutContext() {}

void CheckoutContext::start(RenameLock&& renameLock) {
  renameLock_ = std::move(renameLock);
}

vector<CheckoutConflict> CheckoutContext::finish(Hash newSnapshot) {
  // Update the in-memory snapshot ID
  parentsLock_->setParents(newSnapshot);

  // Release our locks.
  // This would release automatically when the CheckoutContext is destroyed,
  // but go ahead and explicitly unlock them just to make sure that we are
  // really completely finished when we fulfill the checkout futures.
  renameLock_.unlock();
  parentsLock_.unlock();

  // Return conflicts_ via a move operation.  We don't need them any more, and
  // can give ownership directly to our caller.
  return std::move(*conflicts_.wlock());
}

void CheckoutContext::addConflict(ConflictType type, RelativePathPiece path) {
  // Errors should be added using addError()
  CHECK(type != ConflictType::ERROR)
      << "attempted to add error using addConflict(): " << path;

  CheckoutConflict conflict;
  conflict.path = path.value().str();
  conflict.type = type;
  conflicts_.wlock()->push_back(std::move(conflict));
}

void CheckoutContext::addConflict(
    ConflictType type,
    TreeInode* parent,
    PathComponentPiece name) {
  // addConflict() should never be called with an unlinked TreeInode.
  //
  // We are holding the RenameLock for the duration of the checkout operation,
  // and we only operate on TreeInode's that still exist in the file system
  // namespace.  Therefore parent->getPath() must always return non-none value
  // here.
  auto parentPath = parent->getPath();
  CHECK(parentPath.hasValue());

  addConflict(type, parentPath.value() + name);
}

void CheckoutContext::addConflict(ConflictType type, InodeBase* inode) {
  // As above, the inode in question must have a path here.
  auto path = inode->getPath();
  CHECK(path.hasValue());
  addConflict(type, path.value());
}

void CheckoutContext::addError(
    TreeInode* parent,
    PathComponentPiece name,
    const folly::exception_wrapper& ew) {
  // As above in addConflict(), the parent tree must have a valid path here.
  auto parentPath = parent->getPath();
  CHECK(parentPath.hasValue());

  auto path = parentPath.value() + name;
  CheckoutConflict conflict;
  conflict.path = path.value().toStdString();
  conflict.type = ConflictType::ERROR;
  conflict.message = folly::exceptionStr(ew).toStdString();
  conflicts_.wlock()->push_back(std::move(conflict));
}
}
}
