/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h>
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/mount/EdenDispatcher.h"
#include "eden/fs/win/mount/FsChannel.h"
#include "eden/fs/win/utils/Guid.h"

namespace facebook {
namespace eden {
class EdenMount;

class PrjfsChannel : public FsChannel {
 public:
  PrjfsChannel(const PrjfsChannel&) = delete;
  PrjfsChannel& operator=(const PrjfsChannel&) = delete;

  explicit PrjfsChannel() = delete;

  PrjfsChannel(EdenMount* mount);
  ~PrjfsChannel();
  void start();
  void stop();

  /**
   * Remove files from the Projected FS cache. removeCachedFile() doesn't care
   * about the file state and will remove file in any state.
   */
  void removeCachedFile(const wchar_t* path) override;

  /**
   * Remove tombstones from the Projected FS cache. Tombstones are Windows
   * reparse points created to keep track of deleted files on the file system.
   * removeDeletedFile() doesn't care about the file state and will remove file
   * in any state.
   */
  void removeDeletedFile(const wchar_t* path) override;

 private:
  static HRESULT CALLBACK startEnumeration(
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId) noexcept;

  static HRESULT CALLBACK endEnumeration(
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId) noexcept;

  static HRESULT CALLBACK getEnumerationData(
      const PRJ_CALLBACK_DATA* callbackData,
      const GUID* enumerationId,
      PCWSTR searchExpression,
      PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) noexcept;

  static HRESULT CALLBACK
  getPlaceholderInfo(const PRJ_CALLBACK_DATA* callbackData) noexcept;

  static HRESULT CALLBACK
  queryFileName(const PRJ_CALLBACK_DATA* callbackData) noexcept;

  static HRESULT CALLBACK getFileData(
      const PRJ_CALLBACK_DATA* callbackData,
      UINT64 byteOffset,
      UINT32 length) noexcept;
  static HRESULT CALLBACK notification(
      const PRJ_CALLBACK_DATA* callbackData,
      BOOLEAN isDirectory,
      PRJ_NOTIFICATION notificationType,
      PCWSTR destinationFileName,
      PRJ_NOTIFICATION_PARAMETERS* notificationParameters) noexcept;

  static void CALLBACK
  cancelOperation(const PRJ_CALLBACK_DATA* callbackData) noexcept;

  void deleteFile(const wchar_t* path, PRJ_UPDATE_TYPES updateFlags);

 private:
  /**
   * getDispatcher fetches the EdenDispatcher from the Projectedfs request.
   */
  static EdenDispatcher* getDispatcher(
      const PRJ_CALLBACK_DATA* callbackData) noexcept;

  /**
   * getDispatcher() return the EdenDispatcher stored with in this object.
   * This object should be same as returned by the getDispatcher() above but is
   * fetched from a different location.
   */
  const EdenDispatcher* getDispatcher() const {
    return &dispatcher_;
  }

  //
  // Channel to talk to projectedFS.
  //
  PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT mountChannel_{nullptr};
  const EdenMount* mount_;

  EdenDispatcher dispatcher_;
  Guid mountId_;
  std::wstring winPath_;
  bool isRunning_{false};
};

} // namespace eden
} // namespace facebook
