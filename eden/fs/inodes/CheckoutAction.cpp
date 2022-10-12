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
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectStore.h"

using folly::exception_wrapper;
using folly::Future;

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
    folly::Future<InodePtr> inodeFuture)
    : ctx_(ctx), inodeFuture_(std::move(inodeFuture)) {
  XDCHECK(oldScmEntry || newScmEntry);
  if (oldScmEntry) {
    oldScmEntry_ = *oldScmEntry;
  }
  if (newScmEntry) {
    newScmEntry_ = *newScmEntry;
  }
}

CheckoutAction::~CheckoutAction() {}

PathComponentPiece CheckoutAction::getEntryName() const {
  XDCHECK(oldScmEntry_.has_value() || newScmEntry_.has_value());
  return oldScmEntry_.has_value() ? oldScmEntry_.value().first
                                  : newScmEntry_.value().first;
}

class CheckoutAction::LoadingRefcount {
 public:
  explicit LoadingRefcount(CheckoutAction* action) : action_(action) {
    action_->numLoadsPending_.fetch_add(1);
  }
  LoadingRefcount(LoadingRefcount&& other) noexcept : action_(other.action_) {
    other.action_ = nullptr;
  }
  LoadingRefcount& operator=(LoadingRefcount&& other) noexcept {
    decref();
    action_ = other.action_;
    other.action_ = nullptr;
    return *this;
  }
  ~LoadingRefcount() {
    decref();
  }

  /**
   * Implement the arrow operator, so that LoadingRefcount can be used like a
   * pointer.  This allows users to easily call through it into the underlying
   * CheckoutAction methods.
   */
  CheckoutAction* operator->() const {
    return action_;
  }

 private:
  void decref() {
    if (action_) {
      auto oldCount = action_->numLoadsPending_.fetch_sub(1);
      if (oldCount == 1) {
        // We were the last load to complete.  We can perform the action now.
        action_->allLoadsComplete();
      }
    }
  }

  CheckoutAction* action_;
};

Future<InvalidationRequired> CheckoutAction::run(
    CheckoutContext* ctx,
    ObjectStore* store) {
  // Immediately create one LoadingRefcount, to ensure that our
  // numLoadsPending_ refcount does not drop to 0 until after we have started
  // all required load operations.
  //
  // Even if all loads complete immediately, allLoadsComplete() won't be called
  // until this LoadingRefcount is destroyed.
  LoadingRefcount refcount{this};

  try {
    // Load the Blob or Tree for the old TreeEntry.
    if (oldScmEntry_.has_value()) {
      const auto& oldEntry = oldScmEntry_.value();
      if (oldEntry.second.isTree()) {
        store->getTree(oldEntry.second.getHash(), ctx->getFetchContext())
            .thenValue([rc = LoadingRefcount(this)](
                           std::shared_ptr<const Tree> oldTree) {
              rc->setOldTree(std::move(oldTree));
            })
            .semi()
            .via(&folly::QueuedImmediateExecutor::instance())
            .thenError([rc = LoadingRefcount(this)](exception_wrapper&& ew) {
              rc->error("error getting old tree", std::move(ew));
            });
      } else {
        store->getBlobSha1(oldEntry.second.getHash(), ctx->getFetchContext())
            .thenValue([rc = LoadingRefcount(this)](Hash20 oldBlobSha1) {
              rc->setOldBlob(std::move(oldBlobSha1));
            })
            .semi()
            .via(&folly::QueuedImmediateExecutor::instance())
            .thenError([rc = LoadingRefcount(this)](exception_wrapper&& ew) {
              rc->error("error getting old blob Sha1", std::move(ew));
            });
      }
    }

    // If we have a new TreeEntry, load the corresponding Blob or Tree
    if (newScmEntry_.has_value()) {
      const auto& newEntry = newScmEntry_.value();
      if (newEntry.second.isTree()) {
        store->getTree(newEntry.second.getHash(), ctx->getFetchContext())
            .thenValue([rc = LoadingRefcount(this)](
                           std::shared_ptr<const Tree> newTree) {
              rc->setNewTree(std::move(newTree));
            })
            .semi()
            .via(&folly::QueuedImmediateExecutor::instance())
            .thenError([rc = LoadingRefcount(this)](exception_wrapper&& ew) {
              rc->error("error getting new tree", std::move(ew));
            });
      } else {
        // We don't actually compare the new blob to anything, so we don't need
        // to fetch it. This just marks that the new inode will be a file.
        LoadingRefcount(this)->setNewBlob();
      }
    }

    // If we were constructed with a Future<InodePtr>, wait for it.
    if (!inode_) {
      XCHECK(inodeFuture_.valid());
      std::move(inodeFuture_)
          .thenValue([rc = LoadingRefcount(this)](InodePtr inode) {
            rc->setInode(std::move(inode));
          })
          .thenError([rc = LoadingRefcount(this)](exception_wrapper&& ew) {
            rc->error("error getting inode", std::move(ew));
          });
    }
  } catch (...) {
    auto ew = exception_wrapper{std::current_exception()};
    refcount->error(
        "error preparing to load data for checkout action", std::move(ew));
  }

  return promise_.getFuture();
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
  XLOG(ERR) << "error performing checkout action: " << msg << ": " << ew;
  errors_.push_back(std::move(ew));
}

void CheckoutAction::allLoadsComplete() noexcept {
  if (!ensureDataReady()) {
    // ensureDataReady() will fulfilled promise_ with an exception
    return;
  }

  try {
    doAction().thenTry([this](folly::Try<InvalidationRequired>&& t) {
      this->promise_.setTry(std::move(t));
    });
  } catch (...) {
    auto ew = exception_wrapper{std::current_exception()};
    promise_.setException(std::move(ew));
  }
}

bool CheckoutAction::ensureDataReady() noexcept {
  if (!errors_.empty()) {
    // If multiple errors occurred, we log them all, but only propagate
    // up the first one.  If necessary we could change this to create
    // a single exception that contains all of the messages concatenated
    // together.
    if (errors_.size() > 1) {
      XLOG(ERR) << "multiple errors while attempting to load data for "
                   "checkout action:";
      for (const auto& ew : errors_) {
        XLOG(ERR) << "CheckoutAction error: " << folly::exceptionStr(ew);
      }
    }
    promise_.setException(errors_[0]);
    return false;
  }

  // Make sure we actually have all the data we need.
  // (Just in case something went wrong when wiring up the callbacks in such a
  // way that we also failed to call error().)
  if (oldScmEntry_.has_value() && (!oldTree_ && !oldBlobSha1_)) {
    promise_.setException(
        std::runtime_error("failed to load data for old TreeEntry"));
    return false;
  }
  if (newScmEntry_.has_value() && (!newTree_ && !newBlobMarker_)) {
    promise_.setException(
        std::runtime_error("failed to load data for new TreeEntry"));
    return false;
  }
  if (!inode_) {
    promise_.setException(std::runtime_error("failed to load affected inode"));
    return false;
  }

  return true;
}

Future<InvalidationRequired> CheckoutAction::doAction() {
  // All the data is ready and we're ready to go!

  // Check for conflicts first.
  return hasConflict().thenValue(
      [this](
          bool conflictWasAddedToCtx) -> folly::Future<InvalidationRequired> {
        // Note that even if we know we are not going to apply the changes, we
        // must still run hasConflict() first because we rely on its
        // side-effects.
        if (conflictWasAddedToCtx && !ctx_->forceUpdate()) {
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
        auto parent = inode_->getParent(ctx_->renameLock());
        return parent->checkoutUpdateEntry(
            ctx_,
            getEntryName(),
            std::move(inode_),
            std::move(oldTree_),
            std::move(newTree_),
            newScmEntry_);
      });
}

Future<bool> CheckoutAction::hasConflict() {
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
            oldScmEntry_.value().second.getHash(),
            oldBlobSha1_.value(),
            oldScmEntry_.value().second.getType(),
            ctx_->getFetchContext())
        .thenValue([this](bool isSame) {
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
          auto conflictType = newScmEntry_ ? ConflictType::MODIFIED_MODIFIED
                                           : ConflictType::MODIFIED_REMOVED;
          ctx_->addConflict(conflictType, inode_.get());
          return true;
        })
        .semi()
        .via(&folly::QueuedImmediateExecutor::instance());
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
