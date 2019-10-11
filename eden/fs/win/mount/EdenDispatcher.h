/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h>
#include <cstdint>
#include <cstring>
#include <map>
#include <string>
#include "eden/fs/win/mount/Enumerator.h"
#include "eden/fs/win/store/WinStore.h"
#include "eden/fs/win/utils/Guid.h"
#include "folly/Synchronized.h"

constexpr uint32_t kDispatcherCode = 0x1155aaff;

namespace facebook {
namespace eden {
class EdenMount;

class EdenDispatcher {
 public:
  explicit EdenDispatcher(EdenMount& mount);

  HRESULT startEnumeration(
      const PRJ_CALLBACK_DATA& callbackData,
      const GUID& enumerationId) noexcept;

  HRESULT getEnumerationData(
      const PRJ_CALLBACK_DATA& callbackData,
      const GUID& enumerationId,
      PCWSTR searchExpression,
      PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) noexcept;

  void endEnumeration(const GUID& enumerationId) noexcept;

  HRESULT
  getFileInfo(const PRJ_CALLBACK_DATA& callbackData) noexcept;

  HRESULT
  getFileData(
      const PRJ_CALLBACK_DATA& callbackData,
      uint64_t byteOffset,
      uint32_t length) noexcept;

  //
  // Pointer to the dispatcher will be returned from the underlying file system.
  //  isValidDispatcher() can be used to verify that it is a correct pointer.

  bool isValidDispatcher() const {
    return (verificationCode_ == kDispatcherCode);
  }

 private:
  HRESULT
  readSingleFileChunk(
      PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
      const GUID& dataStreamId,
      const folly::IOBuf& iobuf,
      uint64_t startOffset,
      uint32_t writeLength);

  HRESULT
  readMultipleFileChunks(
      PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
      const GUID& dataStreamId,
      const folly::IOBuf& iobuf,
      uint64_t startOffset,
      uint32_t length,
      uint32_t writeLength);

  // Store a raw pointer to EdenMount. It doesn't own or maintain the lifetime
  // of Mount. Instead, at this point, Eden dispatcher is owned by the
  // mount.
  EdenMount& mount_;
  EdenMount& getMount() {
    return mount_;
  }

  WinStore winStore_;

  //
  //  This will have a list of currently active enumeration sessions
  //
  // TODO: add the hash function and convert it to unordered_map (may be).
  folly::Synchronized<std::map<GUID, std::unique_ptr<Enumerator>, CompareGuid>>
      enumSessions_;

  const uint32_t verificationCode_ = kDispatcherCode;
};

} // namespace eden
} // namespace facebook
