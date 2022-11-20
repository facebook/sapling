/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/portability/Windows.h>

#include "eden/fs/prjfs/Enumerator.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/RefPtr.h"

namespace facebook::eden {

class ObjectFetchContext;
class EdenStats;
template <class T>
class ImmediateFuture;

using ObjectFetchContextPtr = RefPtr<ObjectFetchContext>;

struct LookupResult {
  // To ensure that the OS has a record of the canonical file name, and not
  // just whatever case was used to lookup the file, we capture the
  // relative path here.
  RelativePath path;
  size_t size;
  bool isDir;
};

class PrjfsDispatcher {
 public:
  virtual ~PrjfsDispatcher();
  explicit PrjfsDispatcher(EdenStats* stats);

  EdenStats* getStats() const;

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
   */
  virtual ImmediateFuture<folly::Unit> fileCreated(
      RelativePath path,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a directory was created
   */
  virtual ImmediateFuture<folly::Unit> dirCreated(
      RelativePath path,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file has been modified
   */
  virtual ImmediateFuture<folly::Unit> fileModified(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file is renamed
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
   */
  virtual ImmediateFuture<folly::Unit> preFileRename(
      RelativePath oldPath,
      RelativePath newPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file was removed
   */
  virtual ImmediateFuture<folly::Unit> fileDeleted(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a file is about to be removed.
   *
   * This should succeed or fail without any side effects to the inode
   * hierarchy.
   */
  virtual ImmediateFuture<folly::Unit> preFileDelete(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a directory was removed
   */
  virtual ImmediateFuture<folly::Unit> dirDeleted(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Notification sent when a directory is about to be removed.
   *
   * This should succeed or fail without any side effects to the inode
   * hierarchy.
   */
  virtual ImmediateFuture<folly::Unit> preDirDelete(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Wait for all received notifications to complete.
   */
  virtual ImmediateFuture<folly::Unit> waitForPendingNotifications() = 0;

 private:
  EdenStats* stats_{nullptr};
};
} // namespace facebook::eden
