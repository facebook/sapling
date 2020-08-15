/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h>
#include <folly/Synchronized.h>
#include <folly/container/F14Map.h>
#include <cstdint>
#include <cstring>
#include <string>
#include "eden/fs/win/mount/Enumerator.h"
#include "eden/fs/win/utils/Guid.h"

constexpr uint32_t kDispatcherCode = 0x1155aaff;

namespace facebook {
namespace eden {
class EdenMount;

class EdenDispatcher {
 public:
  explicit EdenDispatcher(EdenMount* mount);

  HRESULT startEnumeration(
      const PRJ_CALLBACK_DATA& callbackData,
      const GUID& enumerationId) noexcept;

  HRESULT getEnumerationData(
      const PRJ_CALLBACK_DATA& callbackData,
      const GUID& enumerationId,
      PCWSTR searchExpression,
      PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) noexcept;

  HRESULT endEnumeration(const GUID& enumerationId) noexcept;

  HRESULT
  getFileInfo(const PRJ_CALLBACK_DATA& callbackData) noexcept;

  HRESULT
  queryFileName(const PRJ_CALLBACK_DATA& callbackData) noexcept;

  HRESULT
  getFileData(
      const PRJ_CALLBACK_DATA& callbackData,
      uint64_t byteOffset,
      uint32_t length) noexcept;

  HRESULT notification(
      const PRJ_CALLBACK_DATA& callbackData,
      bool isDirectory,
      PRJ_NOTIFICATION notificationType,
      PCWSTR destinationFileName,
      PRJ_NOTIFICATION_PARAMETERS& notificationParameters) noexcept;

  //
  // Pointer to the dispatcher will be returned from the underlying file system.
  // isValidDispatcher() can be used to verify that it is a correct pointer.
  //

  bool isValidDispatcher() const {
    return (verificationCode_ == kDispatcherCode);
  }

 private:
  // The EdenMount that owns this EdenDispatcher.
  EdenMount* const mount_;

  // Set of currently active directory enumerations.
  folly::Synchronized<folly::F14FastMap<Guid, Enumerator>> enumSessions_;

  const std::string dotEdenConfig_;

  const uint32_t verificationCode_ = kDispatcherCode;
};

} // namespace eden
} // namespace facebook
