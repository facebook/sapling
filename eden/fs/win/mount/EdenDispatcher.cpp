/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <folly/Format.h>
#include <folly/logging/xlog.h>
#include "ProjectedFSLib.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/service/EdenError.h"
#include "eden/fs/win/mount/EdenDispatcher.h"
#include "eden/fs/win/mount/EdenMount.h"
#include "eden/fs/win/store/WinStore.h"
#include "eden/fs/win/utils/StringConv.h"
#include "eden/fs/win/utils/WinError.h"

using folly::sformat;
using std::make_unique;
using std::wstring;

namespace facebook {
namespace eden {

namespace {
struct PrjAlignedBufferDeleter {
  void operator()(void* buffer) noexcept {
    ::PrjFreeAlignedBuffer(buffer);
  }
};
} // namespace

constexpr uint32_t kMinChunkSize = 512 * 1024; // 512 KB
constexpr uint32_t kMaxChunkSize = 5 * 1024 * 1024; // 5 MB

#ifdef NDEBUG
// Some of the following functions will be called with a high frequency.
// Creating a way to totally take out the calls in the free builds.
#define TRACE(fmt, ...)
#else
#define TRACE(fmt, ...) XLOG(DBG6) << sformat(fmt, ##__VA_ARGS__)
#endif

EdenDispatcher::EdenDispatcher(EdenMount& mount)
    : mount_{mount}, winStore_{mount} {
  XLOGF(
      INFO,
      "Creating Dispatcher mount (0x{:x}) root ({}) dispatcher (0x{:x})",
      reinterpret_cast<uintptr_t>(&getMount()),
      getMount().getPath(),
      reinterpret_cast<uintptr_t>(this));
}

HRESULT EdenDispatcher::startEnumeration(
    const PRJ_CALLBACK_DATA& callbackData,
    const GUID& enumerationId) noexcept {
  try {
    std::vector<FileMetadata> list;
    wstring path{callbackData.FilePathName};

    TRACE(
        "startEnumeration mount (0x{:x}) root ({}) path ({}) process ({})",
        reinterpret_cast<uintptr_t>(&getMount()),
        getMount().getPath(),
        wstringToString(path),
        wcharToString(callbackData.TriggeringProcessImageFileName));

    if (!winStore_.getAllEntries(path, list)) {
      TRACE("File not found path ({})", wstringToString(path));
      return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
    }

    auto [iterator, inserted] = enumSessions_.wlock()->emplace(
        enumerationId,
        make_unique<Enumerator>(
            enumerationId, std::move(path), std::move(list)));
    DCHECK(inserted);
    return S_OK;
  } catch (const std::exception& ex) {
    return exceptionToHResult();
  }
}

void EdenDispatcher::endEnumeration(const GUID& enumerationId) noexcept {
  try {
    auto erasedCount = enumSessions_.wlock()->erase(enumerationId);
    DCHECK(erasedCount == 1);
  } catch (const std::exception& ex) {
    // Don't need to return result here - exceptionToHResult() will log the
    // error.
    (void)exceptionToHResult();
  }
}

HRESULT EdenDispatcher::getEnumerationData(
    const PRJ_CALLBACK_DATA& callbackData,
    const GUID& enumerationId,
    PCWSTR searchExpression,
    PRJ_DIR_ENTRY_BUFFER_HANDLE bufferHandle) noexcept {
  try {
    //
    // Error if we don't have the session.
    //
    auto lockedSessions = enumSessions_.rlock();
    auto sessionIterator = lockedSessions->find(enumerationId);
    if (sessionIterator == lockedSessions->end()) {
      XLOG(DBG5) << "Enum instance not found: "
                 << wstringToString(callbackData.FilePathName);
      return HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER);
    }

    auto shouldRestart =
        bool(callbackData.Flags & PRJ_CB_DATA_FLAG_ENUM_RESTART_SCAN);
    auto& session = sessionIterator->second;

    if (session->isSearchExpressionEmpty() || shouldRestart) {
      if (searchExpression != nullptr) {
        session->saveExpression(searchExpression);
      } else {
        session->saveExpression(L"*");
      }
    }

    if (shouldRestart) {
      session->restart();
    }

    //
    // Traverse the list enumeration list and fill the remaining entry. Start
    // from where the last call left off.
    //
    for (const FileMetadata* entry; (entry = session->current());
         session->advance()) {
      auto fileInfo = PRJ_FILE_BASIC_INFO();

      fileInfo.IsDirectory = entry->isDirectory;
      fileInfo.FileSize = entry->size;

      TRACE(
          "Enum {} {} size= {}",
          wstringToString(entry->name),
          fileInfo.IsDirectory ? "Dir" : "File",
          fileInfo.FileSize);

      if (S_OK !=
          PrjFillDirEntryBuffer(entry->name.c_str(), &fileInfo, bufferHandle)) {
        // We are out of buffer space. This entry didn't make it. Return without
        // increment.
        return S_OK;
      }
    }
    return S_OK;
  } catch (const std::exception& ex) {
    return exceptionToHResult();
  }
}

HRESULT
EdenDispatcher::getFileInfo(const PRJ_CALLBACK_DATA& callbackData) noexcept {
  try {
    PRJ_PLACEHOLDER_INFO placeholderInfo = {};
    const wstring path{callbackData.FilePathName};
    FileMetadata metadata = {};

    if (!winStore_.getFileMetadata(path, metadata)) {
      TRACE("{} : File not Found", wstringToString(path));
      return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
    }

    TRACE(
        "Found {} {} size= {} process {}",
        wstringToString(metadata.name),
        metadata.isDirectory ? "Dir" : "File",
        metadata.size,
        wcharToString(callbackData.TriggeringProcessImageFileName));

    placeholderInfo.FileBasicInfo.IsDirectory = metadata.isDirectory;
    placeholderInfo.FileBasicInfo.FileSize = metadata.size;

    //
    // Don't use "metadata.name.c_str()" for the second argument in
    // PrjWritePlaceholderInfo. That seems to throw internal error for FS create
    // operation.
    //

    HRESULT result = PrjWritePlaceholderInfo(
        callbackData.NamespaceVirtualizationContext,
        callbackData.FilePathName,
        &placeholderInfo,
        sizeof(placeholderInfo));
    if (FAILED(result)) {
      XLOGF(
          DBG2,
          "Failed to send the file info. file {} error {} msg {}",
          wstringToString(path),
          result,
          win32ErrorToString(result));
    }
    return result;
  } catch (const std::exception& ex) {
    return exceptionToHResult();
  }
}

static uint64_t BlockAlignTruncate(uint64_t ptr, uint32_t alignment) {
  return ((ptr) & (0 - (static_cast<uint64_t>(alignment))));
}

HRESULT
EdenDispatcher::getFileData(
    const PRJ_CALLBACK_DATA& callbackData,
    uint64_t byteOffset,
    uint32_t length) noexcept {
  try {
    //
    // We should return file data which is smaller than
    // our kMaxChunkSize and meets the memory alignment requirements
    // of the virtualization instance's storage device.
    //
    auto blob = winStore_.getBlob(callbackData.FilePathName);
    if (!blob) {
      return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
    }

    const folly::IOBuf& iobuf = blob->getContents();
    //
    // Assuming that it will not be a chain of IOBUFs.
    // TODO: The following assert fails - need to dig more into IOBuf.
    // assert(iobuf.next() == nullptr);

    if (iobuf.length() <= kMinChunkSize) {
      //
      // If the file is small - copy the whole file in one shot.
      //
      return readSingleFileChunk(
          callbackData.NamespaceVirtualizationContext,
          callbackData.DataStreamId,
          iobuf,
          /*startOffset=*/0,
          /*writeLength=*/iobuf.length());

    } else if (length <= kMaxChunkSize) {
      //
      // If the request is with in our kMaxChunkSize - copy the entire request.
      //
      return readSingleFileChunk(
          callbackData.NamespaceVirtualizationContext,
          callbackData.DataStreamId,
          iobuf,
          /*startOffset=*/byteOffset,
          /*writeLength=*/length);
    } else {
      //
      // When the request is larger than kMaxChunkSize we split the
      // request into multiple chunks.
      //
      PRJ_VIRTUALIZATION_INSTANCE_INFO instanceInfo;
      HRESULT result = PrjGetVirtualizationInstanceInfo(
          callbackData.NamespaceVirtualizationContext, &instanceInfo);

      if (FAILED(result)) {
        return result;
      }

      uint64_t startOffset = byteOffset;
      uint64_t endOffset = BlockAlignTruncate(
          startOffset + kMaxChunkSize, instanceInfo.WriteAlignment);
      DCHECK(endOffset > 0);
      DCHECK(endOffset > startOffset);

      uint32_t chunkSize = endOffset - startOffset;
      return readMultipleFileChunks(
          callbackData.NamespaceVirtualizationContext,
          callbackData.DataStreamId,
          iobuf,
          /*startOffset=*/startOffset,
          /*length=*/length,
          /*chunkSize=*/chunkSize);
    }
  } catch (const std::exception& ex) {
    return exceptionToHResult();
  }
}

HRESULT
EdenDispatcher::readSingleFileChunk(
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
    const GUID& dataStreamId,
    const folly::IOBuf& iobuf,
    uint64_t startOffset,
    uint32_t length) {
  return readMultipleFileChunks(
      namespaceVirtualizationContext,
      dataStreamId,
      iobuf,
      /*startOffset=*/startOffset,
      /*length=*/length,
      /*writeLength=*/length);
}

HRESULT
EdenDispatcher::readMultipleFileChunks(
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
    const GUID& dataStreamId,
    const folly::IOBuf& iobuf,
    uint64_t startOffset,
    uint32_t length,
    uint32_t chunkSize) {
  HRESULT result;
  std::unique_ptr<void, PrjAlignedBufferDeleter> writeBuffer{
      PrjAllocateAlignedBuffer(namespaceVirtualizationContext, chunkSize)};

  if (writeBuffer.get() == nullptr) {
    return E_OUTOFMEMORY;
  }

  uint32_t remainingLength = length;

  while (remainingLength > 0) {
    uint32_t copySize = std::min(remainingLength, chunkSize);

    //
    // TODO(puneetk): Once backing store has the support for chunking the file
    // contents, we can read the chunks of large files here and then write
    // them to FS.
    //
    // TODO(puneetk): Build an interface to backing store so that we can pass
    // the aligned buffer to avoid coping here.
    //
    RtlCopyMemory(writeBuffer.get(), iobuf.data() + startOffset, copySize);

    // Write the data to the file in the local file system.
    result = PrjWriteFileData(
        namespaceVirtualizationContext,
        &dataStreamId,
        writeBuffer.get(),
        startOffset,
        copySize);

    if (FAILED(result)) {
      return result;
    }

    remainingLength -= copySize;
    startOffset += copySize;
  }

  return S_OK;
}
} // namespace eden
} // namespace facebook
