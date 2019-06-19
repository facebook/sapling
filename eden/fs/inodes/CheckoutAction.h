/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/futures/Future.h>
#include <memory>
#include <optional>
#include <vector>
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/model/TreeEntry.h"

namespace folly {
class exception_wrapper;
}

namespace facebook {
namespace eden {

class Blob;
class CheckoutContext;
class ObjectStore;
class Tree;

enum class InvalidationRequired : bool {
  No,
  Yes,
};

/**
 * A helper class representing an action that must be taken as part of a
 * checkout operation.
 *
 * The TreeInode is responsible for computing the list of CheckoutActions that
 * must be run in order to perform a checkout.  These actions are computed
 * while holding the TreeInode's contents_ lock, and then executed after
 * releasing the lock.
 *
 * A few actions can be done immediately while still holding the TreeInode's
 * contents lock.  In particular, this includes creating new entries for files
 * or directories that did not previously exist.  TreeInode is responsible for
 * performing these actions while still holding the contents_ lock.  No
 * CheckoutAction objects are ever created for these cases, since these actions
 * can be taken immediately.
 */
class CheckoutAction {
 public:
  /**
   * Create a CheckoutAction with an already loaded Inode object.
   */
  CheckoutAction(
      CheckoutContext* ctx,
      const TreeEntry* oldScmEntry,
      const TreeEntry* newScmEntry,
      InodePtr&& inode);

  /**
   * Create a CheckoutAction where the Inode object in question is not loaded
   * yet.
   *
   * (This is a template function purely to avoid ambiguity with the
   * constructor type above.  Future<InodePtr> is implicitly constructible from
   * an InodePtr, but we want to prefer the constructor above if we have an
   * InodePtr.)
   */
  template <typename InodePtrType>
  CheckoutAction(
      CheckoutContext* ctx,
      const TreeEntry* oldScmEntry,
      const TreeEntry* newScmEntry,
      folly::Future<InodePtrType> inodeFuture)
      : CheckoutAction(
            INTERNAL,
            ctx,
            oldScmEntry,
            newScmEntry,
            std::move(inodeFuture)) {}

  /*
   * CheckoutAction does not allow copying or moving.
   *
   * We hold a pointer to ourself while waiting on the data to load, so we
   * cannot allow the object to potentially move to another address.
   */
  CheckoutAction(CheckoutAction&& other) = delete;
  CheckoutAction& operator=(CheckoutAction&& other) = delete;

  ~CheckoutAction();

  PathComponentPiece getEntryName() const;

  /**
   * Run the CheckoutAction.
   *
   * If this completes successfully, the result returned via the Future
   * indicates if the change updated the parent directory's entries. Returns
   * whether the caller is responsible for invalidating the directory's inode
   * cache in the kernel.
   */
  FOLLY_NODISCARD folly::Future<InvalidationRequired> run(
      CheckoutContext* ctx,
      ObjectStore* store);

 private:
  class LoadingRefcount;

  enum InternalConstructor {
    INTERNAL,
  };
  CheckoutAction(
      InternalConstructor,
      CheckoutContext* ctx,
      const TreeEntry* oldScmEntry,
      const TreeEntry* newScmEntry,
      folly::Future<InodePtr> inodeFuture);

  void setOldTree(std::shared_ptr<const Tree> tree);
  void setOldBlob(std::shared_ptr<const Blob> blob);
  void setNewTree(std::shared_ptr<const Tree> tree);
  void setNewBlob(std::shared_ptr<const Blob> blob);
  void setInode(InodePtr inode);
  void error(folly::StringPiece msg, const folly::exception_wrapper& ew);

  void allLoadsComplete() noexcept;
  bool ensureDataReady() noexcept;
  folly::Future<bool> hasConflict();

  /**
   * Return whether the directory's contents have changed and the
   * inode's readdir cache must be flushed.
   */
  FOLLY_NODISCARD folly::Future<InvalidationRequired> doAction();

  /**
   * The context for the in-progress checkout operation.
   */
  CheckoutContext* const ctx_{nullptr};

  /**
   * The TreeEntry in the old Tree that we are moving away from.
   *
   * This will be none if the entry did not exist in the old Tree.
   */
  std::optional<TreeEntry> oldScmEntry_;

  /**
   * The TreeEntry in the new Tree that we are checking out.
   *
   * This will be none if the entry is deleted in the new Tree.
   */
  std::optional<TreeEntry> newScmEntry_;

  /**
   * A Future that will be invoked when the inode is loaded.
   *
   * This may be unset if the inode was already available when the
   * CheckoutAction was created (in which case inode_ will be non-null).
   */
  folly::Future<InodePtr> inodeFuture_ = folly::Future<InodePtr>::makeEmpty();

  /**
   * A reference count tracking number of outstanding futures still
   * running as part of the process to load all of our data.
   *
   * When all futures complete (successfully or not) this will drop to zero,
   * at which point allDataReady() will be invoked to complete the action.
   */
  std::atomic<uint32_t> numLoadsPending_{0};

  /*
   * Data that we have to load to perform the checkout action.
   *
   * Only one each oldTree_ and oldBlob_ will be loaded,
   * and the same goes for newTree_ and newBlob_.
   *
   * TODO: We don't actually ever need the data from new blob.  If the
   * destination entry is a blob we could just record this fact, and not bother
   * loading the blob data itself.
   */
  InodePtr inode_;
  std::shared_ptr<const Tree> oldTree_;
  std::shared_ptr<const Blob> oldBlob_;
  std::shared_ptr<const Tree> newTree_;
  std::shared_ptr<const Blob> newBlob_;

  /**
   * The errors vector keeps track of any errors that occurred while trying to
   * load the data needed to perform the checkout action.
   */
  std::vector<folly::exception_wrapper> errors_;

  /**
   * The promise that we will fulfil when the CheckoutAction is complete.
   */
  folly::Promise<InvalidationRequired> promise_;
};
} // namespace eden
} // namespace facebook
