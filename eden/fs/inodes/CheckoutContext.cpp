/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/CheckoutContext.h"

#include <folly/logging/xlog.h>

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/TreeInode.h"

using folly::Future;
using folly::Unit;
using std::vector;

namespace facebook {
namespace eden {

CheckoutContext::CheckoutContext(
    EdenMount* mount,
    folly::Synchronized<EdenMount::ParentInfo>::LockedPtr&& parentsLock,
    CheckoutMode checkoutMode)
    : checkoutMode_{checkoutMode},
      mount_{mount},
      parentsLock_(std::move(parentsLock)) {}

CheckoutContext::~CheckoutContext() {}

void CheckoutContext::start(RenameLock&& renameLock) {
  renameLock_ = std::move(renameLock);
}

Future<vector<CheckoutConflict>> CheckoutContext::finish(Hash newSnapshot) {
  // Only update the parents if it is not a dry run.
  if (!isDryRun()) {
    // Update the in-memory snapshot ID
    parentsLock_->parents.setParents(newSnapshot);
  }

  // Release the rename lock.
  // This allows any filesystem unlink() or rename() operations to proceed.
  renameLock_.unlock();

  // If we have a FUSE channel, flush all invalidations we sent to the kernel
  // as part of the checkout operation.  This will ensure that other processes
  // will see up-to-date data once we return.
  //
  // We do this after releasing the rename lock since some of the invalidation
  // operations may be blocked waiting on FUSE unlink() and rename() operations
  // complete.
  auto* fuseChannel = mount_->getFuseChannel();
  if (!isDryRun() && fuseChannel) {
    XLOG(DBG4) << "waiting for inode invalidations to complete";
    return fuseChannel->flushInvalidations().thenValue([this](auto&&) {
      XLOG(DBG4) << "finished processing inode invalidations";
      parentsLock_.unlock();
      return std::move(*conflicts_.wlock());
    });
  }

  // Release the parentsLock_.
  // Once this is released other checkout operations may proceed.
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
  CHECK(parentPath.has_value());

  addConflict(type, parentPath.value() + name);
}

void CheckoutContext::addConflict(ConflictType type, InodeBase* inode) {
  // As above, the inode in question must have a path here.
  auto path = inode->getPath();
  CHECK(path.has_value());
  addConflict(type, path.value());
}

void CheckoutContext::addError(
    TreeInode* parent,
    PathComponentPiece name,
    const folly::exception_wrapper& ew) {
  // As above in addConflict(), the parent tree must have a valid path here.
  auto parentPath = parent->getPath();
  CHECK(parentPath.has_value());

  auto path = parentPath.value() + name;
  CheckoutConflict conflict;
  conflict.path = path.value();
  conflict.type = ConflictType::ERROR;
  conflict.message = folly::exceptionStr(ew).toStdString();
  conflicts_.wlock()->push_back(std::move(conflict));
}
} // namespace eden
} // namespace facebook
