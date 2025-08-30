/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/CheckoutAction.h"

#include <folly/logging/xlog.h>

#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectStore.h"

using folly::exception_wrapper;

namespace facebook::eden {

CheckoutAction::CheckoutAction(
    CheckoutContext* ctx,
    const Tree::value_type* oldScmEntry,
    const Tree::value_type* newScmEntry,
    InodePtr&& inode)
    : ctx_(ctx), inode_(std::move(inode)) {
  XDCHECK(oldScmEntry || newScmEntry);
  if (oldScmEntry) {
    oldScmEntry_ = *oldScmEntry;
  }
  if (newScmEntry) {
    newScmEntry_ = *newScmEntry;
  }
}

CheckoutAction::CheckoutAction(
    InternalConstructor,
    CheckoutContext* ctx,
    const Tree::value_type* oldScmEntry,
    const Tree::value_type* newScmEntry,
    ImmediateFuture<InodePtr> inodeFuture)
    : ctx_(ctx), inodeFuture_(std::move(inodeFuture)) {
  XDCHECK(oldScmEntry || newScmEntry);
  if (oldScmEntry) {
    oldScmEntry_ = *oldScmEntry;
  }
  if (newScmEntry) {
    newScmEntry_ = *newScmEntry;
  }
}

CheckoutAction::~CheckoutAction() = default;

PathComponentPiece CheckoutAction::getEntryName() const {
  XDCHECK(oldScmEntry_.has_value() || newScmEntry_.has_value());
  return oldScmEntry_.has_value() ? oldScmEntry_.value().first
                                  : newScmEntry_.value().first;
}

ImmediateFuture<InvalidationRequired> CheckoutAction::run(
    CheckoutContext* ctx,
    ObjectStore* store) {
  std::vector<ImmediateFuture<folly::Unit>> loadFutures;
  try {
    // Load the Blob or Tree for the old TreeEntry.
    if (oldScmEntry_.has_value()) {
      const auto& oldEntry = oldScmEntry_.value();
      if (oldEntry.second.isTree()) {
        loadFutures.emplace_back(
            store
                ->getTree(oldEntry.second.getObjectId(), ctx->getFetchContext())
                .thenValue([self = shared_from_this()](
                               std::shared_ptr<const Tree> oldTree) {
                  self->setOldTree(std::move(oldTree));
                })
                .thenError([self = shared_from_this()](exception_wrapper&& ew) {
                  self->error("error getting old tree", std::move(ew));
                }));
      } else {
        loadFutures.emplace_back(
            store
                ->getBlobSha1(
                    oldEntry.second.getObjectId(), ctx->getFetchContext())
                .thenValue([self = shared_from_this()](Hash20 oldBlobSha1) {
                  self->setOldBlob(std::move(oldBlobSha1));
                })
                .thenError([self = shared_from_this()](exception_wrapper&& ew) {
                  self->error("error getting old blob Sha1", std::move(ew));
                }));
      }
    }

    // If we have a new TreeEntry, load the corresponding Blob or Tree
    if (newScmEntry_.has_value()) {
      const auto& newEntry = newScmEntry_.value();
      if (newEntry.second.isTree()) {
        loadFutures.emplace_back(
            store
                ->getTree(newEntry.second.getObjectId(), ctx->getFetchContext())
                .thenValue([self = shared_from_this()](
                               std::shared_ptr<const Tree> newTree) {
                  self->setNewTree(std::move(newTree));
                })
                .thenError([self = shared_from_this()](exception_wrapper&& ew) {
                  self->error("error getting new tree", std::move(ew));
                }));
      } else {
        // We don't actually compare the new blob to anything, so we don't need
        // to fetch it. This just marks that the new inode will be a file.
        setNewBlob();
      }
    }

    // If we were constructed with a Future<InodePtr>, wait for it.
    if (!inode_) {
      XCHECK(inodeFuture_.valid());
      loadFutures.emplace_back(
          std::move(inodeFuture_)
              .thenValue([self = shared_from_this()](InodePtr inode) {
                self->setInode(std::move(inode));
              })
              .thenError([self = shared_from_this()](exception_wrapper&& ew) {
                self->error("error getting inode", std::move(ew));
              }));
    }
  } catch (...) {
    auto ew = exception_wrapper{std::current_exception()};
    error("error preparing to load data for checkout action", std::move(ew));
  }

  return collectAll(std::move(loadFutures))
      .thenValue(
          [self = shared_from_this()](
              auto&&) -> ImmediateFuture<InvalidationRequired> {
            if (!self->errors_.empty()) {
              // If multiple errors occurred, we log them all, but only
              // propagate up the first one.  If necessary we could change this
              // to create a single exception that contains all of the messages
              // concatenated together.
              XLOG(
                  ERR,
                  "multiple errors while attempting to load data for checkout action:");
              for (const auto& ew : self->errors_) {
                XLOGF(ERR, "CheckoutAction error: {}", folly::exceptionStr(ew));
              }
              return makeImmediateFuture<InvalidationRequired>(
                  self->errors_[0]);
            }

            return self->doAction();
          });
}

void CheckoutAction::setOldTree(std::shared_ptr<const Tree> tree) {
  XCHECK(!oldTree_);
  XCHECK(!oldBlobSha1_);
  oldTree_ = std::move(tree);
}

void CheckoutAction::setOldBlob(Hash20 blobSha1) {
  XCHECK(!oldTree_);
  XCHECK(!oldBlobSha1_);
  oldBlobSha1_ = std::move(blobSha1);
}

void CheckoutAction::setNewTree(std::shared_ptr<const Tree> tree) {
  XCHECK(!newTree_);
  XCHECK(!newBlobMarker_);
  newTree_ = std::move(tree);
}

void CheckoutAction::setNewBlob() {
  XCHECK(!newTree_);
  XCHECK(!newBlobMarker_);
  newBlobMarker_ = true;
}

void CheckoutAction::setInode(InodePtr inode) {
  XCHECK(!inode_);
  inode_ = std::move(inode);
}

void CheckoutAction::error(
    folly::StringPiece msg,
    folly::exception_wrapper&& ew) {
  XLOGF(ERR, "error performing checkout action: {}: {}", msg, ew);
  errors_.push_back(std::move(ew));
}

ImmediateFuture<InvalidationRequired> CheckoutAction::doAction() {
  // All the data is ready and we're ready to go!

  // Check for conflicts first.
  return hasConflict().thenValue(
      [self = shared_from_this()](
          bool conflictWasAddedToCtx) -> ImmediateFuture<InvalidationRequired> {
        // Note that even if we know we are not going to apply the changes, we
        // must still run hasConflict() first because we rely on its
        // side-effects.
        if (conflictWasAddedToCtx && !self->ctx_->forceUpdate()) {
          // Since we aren't doing another checkoutUpdateEntry, the checkout
          // from this inode won't be executed if it is a tree. In that case, we
          // add that to our "completed" checkout for all of its descendants
          auto treeInode = self->inode_.asTreeOrNull();
          auto increase = treeInode ? treeInode->getInMemoryDescendants() : 0;
          self->ctx_->increaseCheckoutCounter(1 + increase);
          // We only report conflicts for files, not directories. The only
          // possible conflict that can occur here if this inode is a TreeInode
          // is that the old source control state was for a file. There aren't
          // really any other conflicts than this to report, even if we recurse.
          // Anything inside this directory is basically just untracked (or
          // possibly ignored) files.
          return InvalidationRequired::No;
        }

        // Call TreeInode::checkoutUpdateEntry() to actually do the work.
        //
        // Note that we are moving most of our state into the
        // checkoutUpdateEntry() arguments.  We have to be slightly careful
        // here: getEntryName() returns a PathComponentPiece that is pointing
        // into a PathComponent owned either by oldScmEntry_ or newScmEntry_.
        // Therefore don't move these scm entries, to make sure we don't
        // invalidate the PathComponentPiece data.
        auto parent = self->inode_->getParent(self->ctx_->renameLock());
        return parent->checkoutUpdateEntry(
            self->ctx_,
            self->getEntryName(),
            std::move(self->inode_),
            std::move(self->oldTree_),
            std::move(self->newTree_),
            self->newScmEntry_);
      });
}

ImmediateFuture<bool> CheckoutAction::hasConflict() {
  if (oldTree_) {
    auto treeInode = inode_.asTreePtrOrNull();
    if (!treeInode) {
      // This was a directory, but has been replaced with a file on disk
      ctx_->addConflict(ConflictType::MODIFIED_MODIFIED, inode_.get());
      return true;
    }

    // TODO: check for permissions changes

    // We don't check if this tree is unmodified from the old tree or not here.
    // We simply apply the checkout to the tree in this case, so that we report
    // conflicts for individual leaf inodes that were modified, and not for the
    // parent directories.
    return false;
  } else if (oldBlobSha1_) {
    auto fileInode = inode_.asFilePtrOrNull();
    if (!fileInode) {
      // This was a file, but has been replaced with a directory on disk
      ctx_->addConflict(ConflictType::MODIFIED_MODIFIED, inode_.get());
      return true;
    }

    // Check that the file contents are the same as the old source control entry
    return fileInode
        ->isSameAs(
            oldScmEntry_.value().second.getObjectId(),
            oldBlobSha1_.value(),
            filteredEntryType(
                oldScmEntry_.value().second.getType(),
                ctx_->getWindowsSymlinksEnabled()),
            ctx_->getFetchContext())
        .thenValue([self = shared_from_this()](bool isSame) {
          if (isSame) {
            // no conflict
            return false;
          }

          // The file contents or mode bits are different:
          // - If the file exists in the new tree but differs from what is
          //   currently in the working copy, then this is a MODIFIED_MODIFIED
          //   conflict.
          // - If the file does not exist in the new tree, then this is a
          //   MODIFIED_REMOVED conflict.
          auto conflictType = self->newScmEntry_
              ? ConflictType::MODIFIED_MODIFIED
              : ConflictType::MODIFIED_REMOVED;
          self->ctx_->addConflict(conflictType, self->inode_.get());
          return true;
        });
  }

  XDCHECK(!oldScmEntry_) << "Both oldTree_ and oldBlob_ are nullptr, "
                            "so this file should not have an oldScmEntry_.";
  XDCHECK(newScmEntry_) << "If there is no oldScmEntry_, then there must be a "
                           "newScmEntry_.";

  auto localIsFile = inode_.asFilePtrOrNull() != nullptr;
  if (localIsFile) {
    auto remoteIsFile = !newScmEntry_->second.isTree();
    if (remoteIsFile) {
      // This entry is a file that did not exist in the old source control tree,
      // but it exists as a tracked file in the new tree.
      ctx_->addConflict(ConflictType::UNTRACKED_ADDED, inode_.get());
      return true;
    } else {
      // This entry is a file that did not exist in the old source control tree,
      // but it exists as a tracked directory in the new tree.
      ctx_->addConflict(ConflictType::MODIFIED_MODIFIED, inode_.get());
      return true;
    }
  } else {
    // This entry is a directory that did not exist in the old source control
    // tree. We must traverse the directory for UNTRACKED_ADDED and
    // MODIFIED_MODIFIED conflicts. Returning false signals that we must
    // recurse into this directory to continue to look for conflicts.
    return false;
  }
}
} // namespace facebook::eden
