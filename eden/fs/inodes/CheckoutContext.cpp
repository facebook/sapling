/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/CheckoutContext.h"

#include <folly/logging/xlog.h>
#include <optional>

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/TreeInode.h"

using folly::Future;
using std::vector;

namespace facebook::eden {

CheckoutContext::CheckoutContext(
    EdenMount* mount,
    CheckoutMode checkoutMode,
    std::optional<pid_t> clientPid,
    folly::StringPiece thriftMethodName,
    const std::unordered_map<std::string, std::string>* requestInfo)
    : checkoutMode_{checkoutMode},
      mount_{mount},
      fetchContext_{
          clientPid,
          ObjectFetchContext::Cause::Thrift,
          thriftMethodName,
          requestInfo} {}

CheckoutContext::~CheckoutContext() {}

void CheckoutContext::start(
    RenameLock&& renameLock,
    EdenMount::ParentLock::LockedPtr&& parentLock,
    RootId newSnapshot,
    std::shared_ptr<const Tree> toTree) {
  renameLock_ = std::move(renameLock);

  // Only update the parent if it is not a dry run.
  if (!isDryRun()) {
    std::optional<RootId> oldParent;
    if (parentLock) {
      XCHECK(parentLock->checkoutInProgress);
      oldParent = parentLock->workingCopyParentRootId;
      // Update the in-memory snapshot ID
      parentLock->checkedOutRootId = newSnapshot;
      parentLock->workingCopyParentRootId = newSnapshot;
      parentLock->checkedOutRootTree = std::move(toTree);
    }

    auto config = mount_->getCheckoutConfig();

    // Save the new snapshot hash to the config
    if (!oldParent.has_value()) {
      config->setCheckedOutCommit(std::move(newSnapshot));
    } else {
      config->setCheckoutInProgress(oldParent.value(), newSnapshot);
    }
    XLOG(DBG1) << "updated snapshot for " << config->getMountPath() << " from "
               << (oldParent.has_value() ? oldParent->value() : "<none>")
               << " to " << newSnapshot;
  }
}

Future<vector<CheckoutConflict>> CheckoutContext::finish(RootId newSnapshot) {
  auto config = mount_->getCheckoutConfig();

  auto parentCommit = config->getParentCommit();
  auto optPid = parentCommit.getInProgressPid();
  if (optPid.has_value() && optPid.value() == folly::get_cached_pid()) {
    XCHECK_EQ(
        parentCommit.getLastCheckoutId(ParentCommit::RootIdPreference::To)
            .value(),
        newSnapshot);
    config->setCheckedOutCommit(newSnapshot);
  }

  // Release the rename lock.
  // This allows any filesystem unlink() or rename() operations to proceed.
  renameLock_.unlock();

  return flush();
}

Future<vector<CheckoutConflict>> CheckoutContext::flush() {
  if (!isDryRun()) {
    // If we have a FUSE channel, flush all invalidations we sent to the kernel
    // as part of the checkout operation.  This will ensure that other processes
    // will see up-to-date data once we return.
    //
    // We do this after releasing the rename lock since some of the invalidation
    // operations may be blocked waiting on FUSE unlink() and rename()
    // operations complete.
    return mount_->flushInvalidations()
        .thenValue([this](auto&&) { return std::move(*conflicts_.wlock()); })
        .semi()
        .via(&folly::QueuedImmediateExecutor::instance());
  }

  // Return conflicts_ via a move operation.  We don't need them any more, and
  // can give ownership directly to our caller.
  return std::move(*conflicts_.wlock());
}

void CheckoutContext::addConflict(ConflictType type, RelativePathPiece path) {
  // Errors should be added using addError()
  XCHECK(type != ConflictType::ERROR)
      << "attempted to add error using addConflict(): " << path;

  CheckoutConflict conflict;
  *conflict.path_ref() = path.value().str();
  *conflict.type_ref() = type;
  conflicts_.wlock()->push_back(std::move(conflict));
}

void CheckoutContext::addConflict(
    ConflictType type,
    TreeInode* parent,
    PathComponentPiece name) {
  // During checkout, updated files and directories are first unlinked before
  // being removed and/or replaced in the DirContents of their parent
  // TreeInode. In between these two, calling addConflict would lead to an
  // unlinked path, thus getPath cannot be used.
  //
  // During checkout, the RenameLock is held without being released, preventing
  // files from being renamed or removed.
  auto parentPath = parent->getUnsafePath();

  addConflict(type, parentPath + name);
}

void CheckoutContext::addConflict(ConflictType type, InodeBase* inode) {
  // See above for why getUnsafePath must be used.
  auto path = inode->getUnsafePath();
  addConflict(type, path);
}

void CheckoutContext::addError(
    TreeInode* parent,
    PathComponentPiece name,
    const folly::exception_wrapper& ew) {
  // See above for why getUnsafePath must be used.
  auto parentPath = parent->getUnsafePath();

  auto path = parentPath + name;
  CheckoutConflict conflict;
  *conflict.path_ref() = path.value();
  *conflict.type_ref() = ConflictType::ERROR;
  *conflict.message_ref() = folly::exceptionStr(ew).toStdString();
  conflicts_.wlock()->push_back(std::move(conflict));
}
} // namespace facebook::eden
