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

#include <folly/Optional.h>
#include <folly/futures/Future.h>
#include <memory>
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
   * Create a CheckoutAction to modify an existing inode.
   */
  CheckoutAction(
      CheckoutContext* ctx,
      const TreeEntry& oldScmEntry,
      const TreeEntry& newScmEntry,
      InodePtr&& inode);

  /**
   * Create a CheckoutAction to remove an existing inode.
   */
  CheckoutAction(
      CheckoutContext* ctx,
      const TreeEntry& oldScmEntry,
      InodePtr&& inode);

  /**
   * Create a CheckoutAction to modify an existing inode, where the Inode
   * object in question is not loaded yet.
   *
   * (This is a template function purely to avoid ambiguity with the
   * constructor type above.  Future<InodePtr> is implicitly constructible from
   * an InodePtr, but we want to prefer the constructor above if we have an
   * InodePtr.)
   */
  template <typename InodePtrType>
  CheckoutAction(
      CheckoutContext* ctx,
      const TreeEntry& oldScmEntry,
      const TreeEntry& newScmEntry,
      folly::Future<InodePtrType> inodeFuture)
      : CheckoutAction(
            INTERNAL,
            ctx,
            oldScmEntry,
            newScmEntry,
            std::move(inodeFuture)) {}

  /**
   * Create a CheckoutAction to remove an existing inode, where the Inode
   * object in question is not loaded yet.
   *
   * (This is a template function purely to avoid ambiguity with the
   * constructor type above.  Future<InodePtr> is implicitly constructible from
   * an InodePtr, but we want to prefer the constructor above if we have an
   * InodePtr.)
   */
  template <typename InodePtrType>
  CheckoutAction(
      CheckoutContext* ctx,
      const TreeEntry& oldScmEntry,
      folly::Future<InodePtrType> inodeFuture)
      : CheckoutAction(
            INTERNAL,
            ctx,
            oldScmEntry,
            folly::none,
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

  folly::Future<folly::Unit> run(CheckoutContext* ctx, ObjectStore* store);

 private:
  class LoadingRefcount;

  enum InternalConstructor {
    INTERNAL,
  };
  CheckoutAction(
      InternalConstructor,
      CheckoutContext* ctx,
      const TreeEntry& oldScmEntry,
      const folly::Optional<TreeEntry>& newScmEntry,
      folly::Future<InodePtr> inodeFuture);

  void setOldTree(std::unique_ptr<Tree> tree);
  void setOldBlob(std::unique_ptr<Blob> blob);
  void setNewTree(std::unique_ptr<Tree> tree);
  void setNewBlob(std::unique_ptr<Blob> blob);
  void setInode(InodePtr inode);
  void error(folly::StringPiece msg, const folly::exception_wrapper& ew);

  void allLoadsComplete() noexcept;
  bool ensureDataReady() noexcept;
  bool hasConflict();
  folly::Future<folly::Unit> doAction();
  folly::Future<folly::Unit> performTreeCheckout();
  folly::Future<folly::Unit> performBlobCheckout();
  folly::Future<folly::Unit> performRemoval();

  /**
   * The context for the in-progress checkout operation.
   */
  CheckoutContext* const ctx_{nullptr};

  /**
   * The TreeEntry in the old Tree that we are moving away from.
   *
   * This is always set.  The checkout code can always immediately create new
   * inode entries for new children; it never has to defer action for them.
   */
  TreeEntry oldScmEntry_;

  /**
   * The TreeEntry in the new Tree that we are checking out.
   *
   * This will be none if the entry is deleted in the new Tree.
   */
  folly::Optional<TreeEntry> newScmEntry_;

  /**
   * A Future that will be invoked when the inode is loaded.
   *
   * This may be unset if the inode was already available when the
   * CheckoutAction was created (in which case inode_ will be non-null).
   */
  folly::Optional<folly::Future<InodePtr>> inodeFuture_;

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
   */
  InodePtr inode_;
  std::unique_ptr<Tree> oldTree_;
  std::unique_ptr<Blob> oldBlob_;
  std::unique_ptr<Tree> newTree_;
  std::unique_ptr<Blob> newBlob_;

  /**
   * The errors vector keeps track of any errors that occurred while trying to
   * load the data needed to perform the checkout action.
   */
  std::vector<folly::exception_wrapper> errors_;

  /**
   * The promise that we will fulfil when the CheckoutAction is complete.
   */
  folly::Promise<folly::Unit> promise_;
};
}
}
