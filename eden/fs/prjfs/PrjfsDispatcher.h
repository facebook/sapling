/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/executors/SequencedExecutor.h>
#include <folly/portability/Windows.h>

#include "eden/common/utils/Guid.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/common/utils/UnboundedQueueExecutor.h"
#include "eden/fs/inodes/InodeTimestamps.h"
#include "eden/fs/prjfs/Enumerator.h"

namespace facebook::eden {

class PrjfsRequestContext;
class ObjectFetchContext;
class EdenStats;
template <class T>
class ImmediateFuture;

using EdenStatsPtr = RefPtr<EdenStats>;
using ObjectFetchContextPtr = RefPtr<ObjectFetchContext>;

struct LookupResult {
  // To ensure that the OS has a record of the canonical file name, and not
  // just whatever case was used to lookup the file, we capture the
  // relative path here.
  RelativePath path;
  size_t size;
  bool isDir;
  std::optional<std::string> symlinkDestination;
};

class PrjfsDispatcher {
 public:
  virtual ~PrjfsDispatcher();
  explicit PrjfsDispatcher(EdenStatsPtr stats);

  const EdenStatsPtr& getStats() const;

  /**
   * Executor on which all the filesystem write notification will run on.
   *
   * ProjectedFS will send write notifications out of order, these will be
   * handled in this executor.
   */
  folly::Executor::KeepAlive<folly::SequencedExecutor> getNotificationExecutor()
      const;

  /**
   * Get the timestamp of the last time a checkout was performed.
   *
   * This must be monotonically increasing as this timestamp will be used when
   * writing placeholders in the working copy.
   */
  virtual EdenTimestamp getLastCheckoutTime() const = 0;

  /**
   * Open a directory
   */
  virtual ImmediateFuture<std::vector<PrjfsDirEntry>> opendir(
      RelativePath path,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Lookup the specified file and get its attributes.
   */
  virtual ImmediateFuture<std::optional<LookupResult>> lookup(
      RelativePath path,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Test if a file with the given name exist
   */
  virtual ImmediateFuture<bool> access(
      RelativePath path,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Read the file with the given name
   *
   * Returns the entire content of the file at path.
   *
   * In the future, this will return only what's in between offset and
   * offset+length.
   */
  virtual ImmediateFuture<std::string> read(
      RelativePath path,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file was created
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> fileCreated(
      RelativePath path,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a directory was created
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> dirCreated(
      RelativePath path,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file has been modified
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> fileModified(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file is renamed
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> fileRenamed(
      RelativePath oldPath,
      RelativePath newPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a directory is about to be renamed
   *
   * This should succeed or fail without any side effects to the inode
   * hierarchy.
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> preDirRename(
      RelativePath oldPath,
      RelativePath newPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file is about to be renamed
   *
   * This should succeed or fail without any side effects to the inode
   * hierarchy.
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> preFileRename(
      RelativePath oldPath,
      RelativePath newPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file was removed
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> fileDeleted(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file is about to be removed.
   *
   * This should succeed or fail without any side effects to the inode
   * hierarchy.
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> preFileDelete(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a directory was removed
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> dirDeleted(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a directory is about to be removed.
   *
   * This should succeed or fail without any side effects to the inode
   * hierarchy.
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> preDirDelete(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file is about to be converted to a full file.
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> preFileConvertedToFull(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * A file is out of sync on the Filesystem, tell EdenFS to match the state
   * of the file on disk.
   *
   * The caller must guarantee that the dispatcher and EdenMount stay alive
   * until the returned ImmediateFuture complete.
   */
  virtual ImmediateFuture<folly::Unit> matchEdenViewOfFileToFS(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Wait for all received notifications to complete.
   */
  virtual ImmediateFuture<folly::Unit> waitForPendingNotifications() = 0;

 private:
  EdenStatsPtr stats_;

  UnboundedQueueExecutor executor_;
  // All the notifications are dispatched to this executor. The
  // waitForPendingNotifications implementation depends on this being a
  // SequencedExecutor.
  folly::Executor::KeepAlive<folly::SequencedExecutor> notificationExecutor_;
};
} // namespace facebook::eden
