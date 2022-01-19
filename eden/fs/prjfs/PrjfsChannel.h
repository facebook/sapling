/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/portability/Windows.h>

#include <folly/futures/Future.h>

#include <ProjectedFSLib.h> // @manual
#include "eden/fs/prjfs/Enumerator.h"
#include "eden/fs/prjfs/PrjfsDispatcher.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/ProcessAccessLog.h"
#include "eden/fs/utils/Rcu.h"

namespace facebook {
namespace eden {
class EdenMount;
class Notifications;
class PrjfsChannelInner;
class PrjfsRequestContext;

namespace detail {
struct RcuTag;
using RcuLockedPtr = RcuPtr<PrjfsChannelInner, RcuTag>::RcuLockedPtr;
} // namespace detail

class PrjfsChannelInner {
 public:
  PrjfsChannelInner(
      std::unique_ptr<PrjfsDispatcher> dispatcher,
      const folly::Logger* straceLogger,
      ProcessAccessLog& processAccessLog);

  ~PrjfsChannelInner() = default;

  explicit PrjfsChannelInner() = delete;
  PrjfsChannelInner(const PrjfsChannelInner&) = delete;
  PrjfsChannelInner& operator=(const PrjfsChannelInner&) = delete;

  ImmediateFuture<folly::Unit> waitForPendingNotifications();

  /**
   * Start a directory listing.
   *
   * May spawn futures which will extend the lifetime of self.
   */
  HRESULT startEnumeration(
      std::shared_ptr<PrjfsRequestContext> context,
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId);

  /**
   * Terminate a directory listing.
   *
   * May spawn futures which will extend the lifetime of self.
   */
  HRESULT endEnumeration(
      std::shared_ptr<PrjfsRequestContext> context,
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId);

  /**
   * Populate as many directory entries that dirEntryBufferHandle can take.
   *
   * May spawn futures which will extend the lifetime of self.
   */
  HRESULT getEnumerationData(
      std::shared_ptr<PrjfsRequestContext> context,
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId,
      PCWSTR searchExpression,
      PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle);

  /**
   * Obtain the metadata for a given file.
   *
   * May spawn futures which will extend the lifetime of self.
   */
  HRESULT getPlaceholderInfo(
      std::shared_ptr<PrjfsRequestContext> context,
      const PRJ_CALLBACK_DATA* callbackData);

  /**
   * Test whether a given file exist in the repository.
   *
   * May spawn futures which will extend the lifetime of self.
   */
  HRESULT queryFileName(
      std::shared_ptr<PrjfsRequestContext> context,
      const PRJ_CALLBACK_DATA* callbackData);

  /**
   * Read the content of the given file.
   *
   * May spawn futures which will extend the lifetime of self.
   */
  HRESULT getFileData(
      std::shared_ptr<PrjfsRequestContext> context,
      const PRJ_CALLBACK_DATA* callbackData,
      UINT64 byteOffset,
      UINT32 length);

  /**
   * Notifies of state change for the given file.
   *
   * May spawn futures which will extend the lifetime of self.
   */
  HRESULT notification(
      std::shared_ptr<PrjfsRequestContext> context,
      const PRJ_CALLBACK_DATA* callbackData,
      BOOLEAN isDirectory,
      PRJ_NOTIFICATION notificationType,
      PCWSTR destinationFileName,
      PRJ_NOTIFICATION_PARAMETERS* notificationParameters);

  /**
   * Notification sent when a file or directory has been created.
   */
  ImmediateFuture<folly::Unit> newFileCreated(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context);

  /**
   * Notification sent when a file or directory has been replaced.
   */
  ImmediateFuture<folly::Unit> fileOverwritten(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context);

  /**
   * Notification sent when a file has been modified.
   */
  ImmediateFuture<folly::Unit> fileHandleClosedFileModified(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context);

  /**
   * Notification sent when a file or directory has been renamed.
   */
  ImmediateFuture<folly::Unit> fileRenamed(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context);

  /**
   * Notification sent prior to a file or directory being renamed.
   *
   * Called prior to ProjectedFS doing any on-disk checks, and thus if newPath
   * exist on disk, the file rename may later fail. The rename is known to have
   * happened only when fileRenamed is called.
   */
  ImmediateFuture<folly::Unit> preRename(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context);

  /**
   * Notification sent when a file or directory has been removed.
   */
  ImmediateFuture<folly::Unit> fileHandleClosedFileDeleted(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context);

  /**
   * Notification sent prior to a hardlink being created.
   */
  ImmediateFuture<folly::Unit> preSetHardlink(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context);

  ProcessAccessLog& getProcessAccessLog() {
    return processAccessLog_;
  }

  void setMountChannel(PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT channel) {
    mountChannel_ = channel;
  }

  void sendSuccess(
      int32_t commandId,
      PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS* FOLLY_NULLABLE extra);

  void sendError(int32_t commandId, HRESULT error);

 private:
  const folly::Logger& getStraceLogger() const {
    return *straceLogger_;
  }

  void addDirectoryEnumeration(Guid guid, std::vector<PrjfsDirEntry> dirents) {
    auto [iterator, inserted] = enumSessions_.wlock()->emplace(
        std::move(guid), std::make_shared<Enumerator>(std::move(dirents)));
    XDCHECK(inserted);
  }

  std::optional<std::shared_ptr<Enumerator>> findDirectoryEnumeration(
      Guid& guid) {
    auto enumerators = enumSessions_.rlock();
    auto it = enumerators->find(guid);

    if (it == enumerators->end()) {
      return std::nullopt;
    }

    return it->second;
  }

  void removeDirectoryEnumeration(Guid& guid) {
    auto erasedCount = enumSessions_.wlock()->erase(guid);
    XDCHECK(erasedCount == 1);
  }

  // Internal ProjectedFS channel used to communicate with ProjectedFS.
  PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT mountChannel_{nullptr};

  std::unique_ptr<PrjfsDispatcher> dispatcher_;
  const folly::Logger* const straceLogger_{nullptr};

  // The processAccessLog_ is owned by PrjfsChannel which is guaranteed to have
  // its lifetime be longer than that of PrjfsChannelInner.
  ProcessAccessLog& processAccessLog_;

  // Set of currently active directory enumerations.
  folly::Synchronized<folly::F14FastMap<Guid, std::shared_ptr<Enumerator>>>
      enumSessions_;
};

class PrjfsChannel {
 public:
  PrjfsChannel(const PrjfsChannel&) = delete;
  PrjfsChannel& operator=(const PrjfsChannel&) = delete;

  explicit PrjfsChannel() = delete;

  PrjfsChannel(
      AbsolutePathPiece mountPath,
      std::unique_ptr<PrjfsDispatcher> dispatcher,
      const folly::Logger* straceLogger,
      std::shared_ptr<ProcessNameCache> processNameCache,
      Guid guid);

  ~PrjfsChannel();

  void start(bool readOnly, bool useNegativePathCaching);

  /**
   * Wait for all the received notifications to be fully handled.
   *
   * The PrjfsChannel will receive notifications and immediately dispatch the
   * work to a background executor and return S_OK to ProjectedFS to unblock
   * applications writing to the EdenFS repository.
   *
   * Thus an application writing to the repository may have their file creation
   * be successful prior to EdenFS having updated its inode hierarchy. This
   * discrepancy can cause issues in EdenFS for operations that exclusively
   * look at the inode hierarchy such as status/checkout/glob.
   *
   * The returned ImmediateFuture will complete when all the previously
   * received notifications have completed.
   */
  ImmediateFuture<folly::Unit> waitForPendingNotifications();

  /**
   * Stop the PrjfsChannel.
   *
   * The returned future will complete once all the pending callbacks and
   * notifications are completed.
   *
   * PrjfsChannel must not be destructed until the returned future is
   * fulfilled.
   */
  folly::SemiFuture<folly::Unit> stop();

  struct StopData {};
  folly::SemiFuture<StopData> getStopFuture();

  /**
   * Remove a file that has been cached on disk by ProjectedFS. This should be
   * called when the content of a materialized file has changed, typically
   * called during on an `update` operation.
   *
   * This can fail when the underlying file cannot be evicted from ProjectedFS,
   * one example is when the user has locked the file.
   */
  FOLLY_NODISCARD folly::Try<void> removeCachedFile(RelativePathPiece path);

  /**
   * Ensure that the directory is a placeholder so that ProjectedFS will always
   * invoke the opendir/readdir callbacks when the user is listing files in it.
   * This particularly matters for directories that were created by the user to
   * later be committed.
   */
  FOLLY_NODISCARD folly::Try<void> addDirectoryPlaceholder(
      RelativePathPiece path);

  void flushNegativePathCache();

  ProcessAccessLog& getProcessAccessLog() {
    return processAccessLog_;
  }

  /**
   * Copy the inner channel.
   *
   * As long as the returned value is alive, the mount cannot be unmounted.
   * When an unmount is pending, the shared_ptr will be NULL.
   */
  detail::RcuLockedPtr getInner() {
    return inner_.rlock();
  }

 private:
  const AbsolutePath mountPath_;
  Guid mountId_;
  bool useNegativePathCaching_{true};
  folly::Promise<StopData> stopPromise_;

  ProcessAccessLog processAccessLog_;

  RcuPtr<PrjfsChannelInner, detail::RcuTag> inner_;

  // Internal ProjectedFS channel used to communicate with ProjectedFS.
  PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT mountChannel_{nullptr};
};

} // namespace eden
} // namespace facebook
