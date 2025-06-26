/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include <unordered_map>
#include <vector>

#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <folly/stop_watch.h>
#include <gtest/gtest_prod.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/StatsFetchContext.h"

namespace folly {
class exception_wrapper;
struct Unit;
} // namespace folly

namespace facebook::eden {

class CheckoutConflict;
class TreeInode;
class Tree;

template <typename T>
class RingBuffer;

/**
 * CheckoutContext maintains state during a checkout operation.
 */
class CheckoutContext {
 public:
  CheckoutContext(
      EdenMount* mount,
      CheckoutMode checkoutMode,
      OptionalProcessId clientPid,
      folly::StringPiece thriftMethodName,
      bool verifyFilesAfterCheckout,
      size_t verifyEveryNInvalidations,
      size_t maxNumberOfInvlidationsToValidate,
      std::shared_ptr<std::atomic<uint64_t>> checkoutProgress = nullptr,
      const std::unordered_map<std::string, std::string>* requestInfo =
          nullptr);

  ~CheckoutContext();

  /**
   * The list of conflicts that were encountered as well as some sample paths
   * that were invalidated during the checkout.
   * TODO: The invalidated sample paths are used for S439820. It can be deleted
   * when the SEV closed*/
  struct CheckoutConflictsAndInvalidations {
    std::vector<CheckoutConflict> conflicts;
    std::vector<InodeNumber> invalidations;
  };

  /**
   * Returns true if the checkout operation should do a dry run, looking for
   * conflicts without actually updating the inode contents. If it returns
   * false, it should actually update the inodes as part of the checkout.
   */
  bool isDryRun() const {
    // TODO: make this configurable on checkout start
    return checkoutMode_ == CheckoutMode::DRY_RUN;
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
   * forceUpdate() can only return true when isDryRun() is false.
   */
  bool forceUpdate() const {
    return checkoutMode_ == CheckoutMode::FORCE;
  }

  /**
   * Start the checkout operation.
   *
   * As a side effect, this updates the SNAPSHOT file on disk, in the case
   * where EdenFS is killed or crashes during checkout, this allows EdenFS to
   * detect that Mercurial is out of date.
   */
  void start(
      RenameLock&& renameLock,
      EdenMount::ParentLock::LockedPtr&& parentLock,
      RootId newSnapshot,
      std::shared_ptr<const Tree> toTree);

  /**
   * Complete the checkout operation
   *
   * Returns the list of conflicts and errors that were encountered as well as
   some sample paths that were invalidated during the checkout.
   TODO: The invalidations can be used to validate that NFS invalidation is
   working correctly.The invalidated sample paths are used for S439820.
   */
  ImmediateFuture<CheckoutConflictsAndInvalidations> finish(
      const RootId& newSnapshot);

  /**
   * Flush the invalidation if needed.
   *
   * Return the list of conflicts and errors.
   */
  ImmediateFuture<std::vector<CheckoutConflict>> flush();

  void addConflict(ConflictType type, RelativePathPiece path, dtype_t dtype);
  void addConflict(
      ConflictType type,
      TreeInode* parent,
      PathComponentPiece name,
      dtype_t dtype);
  void addConflict(ConflictType type, InodeBase* inode);

  void addError(
      TreeInode* parent,
      PathComponentPiece name,
      const folly::exception_wrapper& ew);

  /**
   * Return this EdenMount's ObjectStore.
   */
  const std::shared_ptr<ObjectStore>& getObjectStore() const {
    return mount_->getObjectStore();
  }

  /**
   * Get a reference to the rename lock.
   *
   * This is mostly used for APIs that require proof that we are currently
   * holding the lock.
   */
  const RenameLock& renameLock() const {
    return renameLock_;
  }

  /**
   * Return the fetch context associated with this checkout context.
   */
  StatsFetchContext& getStatsContext() {
    return *fetchContext_;
  }

  const ObjectFetchContextPtr& getFetchContext() const {
    return fetchContext_.as<ObjectFetchContext>();
  }

  bool getWindowsSymlinksEnabled() const {
    return windowsSymlinksEnabled_;
  }

  void increaseCheckoutCounter(int64_t inc) const;

  void maybeRecordInvalidation(InodeNumber number);

 private:
  FRIEND_TEST(CheckoutContextTest, empty);
  FRIEND_TEST(CheckoutContextTest, overMax);

  std::vector<InodeNumber> extractFilesToVerify();

  CheckoutMode checkoutMode_;
  EdenMount* const mount_;
  RenameLock renameLock_;
  RefPtr<StatsFetchContext> fetchContext_;

  std::shared_ptr<std::atomic<uint64_t>> checkoutProgress_;

  // The checkout processing may occur across many threads,
  // if some data load operations complete asynchronously on other threads.
  // Therefore access to the conflicts list must be synchronized.
  folly::Synchronized<std::vector<CheckoutConflict>> conflicts_;

  bool verifyFilesAfterCheckout_;
  size_t verifyEveryNInvalidations_;
  size_t maxNumberOfInvlidationsToValidate_;
  std::atomic_int64_t invalidationCount_{0};
  folly::Synchronized<std::unique_ptr<RingBuffer<InodeNumber>>>
      sampleInvalidations_;

  bool windowsSymlinksEnabled_;
};
} // namespace facebook::eden
