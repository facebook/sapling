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
#include "eden/fs/win/utils/Guid.h"

namespace facebook {
namespace eden {
class EdenMount;
class EdenDispatcher;

class FsChannel {
 public:
  FsChannel(const FsChannel&) = delete;
  FsChannel& operator=(const FsChannel&) = delete;

  FsChannel() = delete;

  FsChannel(const AbsolutePath& mountPath, EdenMount* mount);
  ~FsChannel();
  void start();
  void stop();

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

 private:
  static EdenDispatcher* getDispatcher(
      const PRJ_CALLBACK_DATA* callbackData) noexcept;

  //
  // Channel to talk to projectedFS.
  //
  PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT mountChannel_{nullptr};
  const EdenMount* mount_;
  Guid mountId_;
  std::wstring winPath_;
  bool isRunning_{false};
};

} // namespace eden
} // namespace facebook
//////////////////////////
