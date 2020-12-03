/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h> // @manual
#include "eden/fs/prjfs/Enumerator.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/ProcessAccessLog.h"
#include "folly/futures/Future.h"

namespace facebook {
namespace eden {
class EdenMount;
class Dispatcher;

class PrjfsChannelInner {
 public:
  PrjfsChannelInner(
      Dispatcher* const dispatcher,
      const folly::Logger* straceLogger,
      ProcessAccessLog& processAccessLog,
      folly::Promise<folly::Unit> deletedPromise);

  ~PrjfsChannelInner();

  explicit PrjfsChannelInner() = delete;
  PrjfsChannelInner(const PrjfsChannelInner&) = delete;
  PrjfsChannelInner& operator=(const PrjfsChannelInner&) = delete;

  HRESULT startEnumeration(
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId);

  HRESULT endEnumeration(
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId);

  HRESULT getEnumerationData(
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId,
      PCWSTR searchExpression,
      PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle);

  HRESULT getPlaceholderInfo(const PRJ_CALLBACK_DATA* callbackData);

  HRESULT queryFileName(const PRJ_CALLBACK_DATA* callbackData);

  HRESULT getFileData(
      const PRJ_CALLBACK_DATA* callbackData,
      UINT64 byteOffset,
      UINT32 length);

  HRESULT notification(
      const PRJ_CALLBACK_DATA* callbackData,
      BOOLEAN isDirectory,
      PRJ_NOTIFICATION notificationType,
      PCWSTR destinationFileName,
      PRJ_NOTIFICATION_PARAMETERS* notificationParameters);

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

  void addDirectoryEnumeration(Guid guid, std::vector<FileMetadata> dirents) {
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

  Dispatcher* const dispatcher_{nullptr};
  const folly::Logger* const straceLogger_{nullptr};

  // The processAccessLog_ is owned by PrjfsChannel which is guaranteed to have
  // its lifetime be longer than that of PrjfsChannelInner.
  ProcessAccessLog& processAccessLog_;

  // Set of currently active directory enumerations.
  folly::Synchronized<folly::F14FastMap<Guid, std::shared_ptr<Enumerator>>>
      enumSessions_;

  // Set when the destructor is called.
  folly::Promise<folly::Unit> deletedPromise_;
};

class PrjfsChannel {
 public:
  PrjfsChannel(const PrjfsChannel&) = delete;
  PrjfsChannel& operator=(const PrjfsChannel&) = delete;

  explicit PrjfsChannel() = delete;

  PrjfsChannel(
      AbsolutePathPiece mountPath,
      Dispatcher* const dispatcher,
      const folly::Logger* straceLogger,
      std::shared_ptr<ProcessNameCache> processNameCache);

  ~PrjfsChannel();

  void start(bool readOnly, bool useNegativePathCaching);

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
  std::shared_ptr<PrjfsChannelInner> getInner() {
    return *inner_.rlock();
  }

 private:
  const AbsolutePath mountPath_;
  Guid mountId_;
  bool useNegativePathCaching_{true};
  folly::Promise<StopData> stopPromise_;

  ProcessAccessLog processAccessLog_;

  folly::SemiFuture<folly::Unit> innerDeleted_;
  folly::Synchronized<std::shared_ptr<PrjfsChannelInner>> inner_;

  // Internal ProjectedFS channel used to communicate with ProjectedFS.
  PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT mountChannel_{nullptr};
};

} // namespace eden
} // namespace facebook
