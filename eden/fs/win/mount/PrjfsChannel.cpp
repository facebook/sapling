/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/mount/PrjfsChannel.h"
#include <folly/logging/xlog.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/mount/EdenDispatcher.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/WinError.h"

using folly::sformat;

namespace {

using facebook::eden::exceptionToHResult;
using facebook::eden::Guid;
using facebook::eden::InodeMetadata;
using facebook::eden::PrjfsChannel;
using facebook::eden::RelativePath;
using facebook::eden::win32ErrorToString;

#define BAIL_ON_RECURSIVE_CALL(callbackData)                               \
  do {                                                                     \
    if (callbackData->TriggeringProcessId == GetCurrentProcessId()) {      \
      auto __path = RelativePath(callbackData->FilePathName);              \
      XLOG(ERR) << "Recursive EdenFS call are disallowed for: " << __path; \
      return HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);                      \
    }                                                                      \
  } while (false)

PrjfsChannel* getChannel(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  DCHECK(callbackData);
  auto channel = static_cast<PrjfsChannel*>(callbackData->InstanceContext);
  DCHECK(channel);
  return channel;
}

HRESULT startEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);

  try {
    auto path = RelativePath(callbackData->FilePathName);
    auto guid = Guid(*enumerationId);
    return getChannel(callbackData)
        ->getDispatcher()
        ->opendir(std::move(path), std::move(guid))
        .thenValue([](auto&&) { return S_OK; })
        .thenError(
            folly::tag_t<std::exception>{},
            [](const std::exception& ex) { return exceptionToHResult(ex); })
        .get();

    return S_OK;
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

HRESULT endEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);
  return getChannel(callbackData)
      ->getDispatcher()
      ->endEnumeration(*enumerationId);
}

HRESULT getEnumerationData(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId,
    PCWSTR searchExpression,
    PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);
  return getChannel(callbackData)
      ->getDispatcher()
      ->getEnumerationData(
          *callbackData,
          *enumerationId,
          searchExpression,
          dirEntryBufferHandle);
}

HRESULT getPlaceholderInfo(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);

  try {
    auto path = RelativePath(callbackData->FilePathName);
    return getChannel(callbackData)
        ->getDispatcher()
        ->lookup(std::move(path))
        .thenValue([context = callbackData->NamespaceVirtualizationContext](
                       const std::optional<InodeMetadata>&& optMetadata) {
          if (!optMetadata) {
            return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
          }
          auto metadata = std::move(optMetadata).value();

          PRJ_PLACEHOLDER_INFO placeholderInfo{};
          placeholderInfo.FileBasicInfo.IsDirectory = metadata.isDir;
          placeholderInfo.FileBasicInfo.FileSize = metadata.size;
          auto inodeName = metadata.path.wide();

          HRESULT result = PrjWritePlaceholderInfo(
              context,
              inodeName.c_str(),
              &placeholderInfo,
              sizeof(placeholderInfo));

          if (FAILED(result)) {
            XLOGF(
                DBG6,
                "{}: {:x} ({})",
                metadata.path,
                result,
                win32ErrorToString(result));
          }

          return result;
        })
        .thenError(
            folly::tag_t<std::exception>{},
            [](const std::exception& ex) { return exceptionToHResult(ex); })
        .get();
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

HRESULT queryFileName(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);

  try {
    auto path = RelativePath(callbackData->FilePathName);
    return getChannel(callbackData)
        ->getDispatcher()
        ->access(std::move(path))
        .thenValue([](bool present) {
          if (present) {
            return S_OK;
          } else {
            return HRESULT(ERROR_FILE_NOT_FOUND);
          }
        })
        .thenError(
            folly::tag_t<std::exception>{},
            [](const std::exception& ex) { return exceptionToHResult(ex); })
        .get();
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

HRESULT getFileData(
    const PRJ_CALLBACK_DATA* callbackData,
    UINT64 byteOffset,
    UINT32 length) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);
  return getChannel(callbackData)
      ->getDispatcher()
      ->getFileData(*callbackData, byteOffset, length);
}

HRESULT notification(
    const PRJ_CALLBACK_DATA* callbackData,
    BOOLEAN isDirectory,
    PRJ_NOTIFICATION notificationType,
    PCWSTR destinationFileName,
    PRJ_NOTIFICATION_PARAMETERS* notificationParameters) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);
  return getChannel(callbackData)
      ->getDispatcher()
      ->notification(
          *callbackData,
          isDirectory,
          notificationType,
          destinationFileName,
          *notificationParameters);
}
} // namespace

namespace facebook {
namespace eden {

PrjfsChannel::PrjfsChannel(
    AbsolutePathPiece mountPath,
    EdenDispatcher* const dispatcher,
    std::shared_ptr<ProcessNameCache> processNameCache)
    : mountPath_(mountPath),
      dispatcher_(dispatcher),
      mountId_(Guid::generate()),
      processAccessLog_(std::move(processNameCache)) {}

PrjfsChannel::~PrjfsChannel() {
  if (isRunning_) {
    stop();
  }
}

void PrjfsChannel::start(bool readOnly, bool useNegativePathCaching) {
  if (readOnly) {
    NOT_IMPLEMENTED();
  }

  auto callbacks = PRJ_CALLBACKS();
  callbacks.StartDirectoryEnumerationCallback = startEnumeration;
  callbacks.EndDirectoryEnumerationCallback = endEnumeration;
  callbacks.GetDirectoryEnumerationCallback = getEnumerationData;
  callbacks.GetPlaceholderInfoCallback = getPlaceholderInfo;
  callbacks.GetFileDataCallback = getFileData;
  callbacks.NotificationCallback = notification;
  callbacks.QueryFileNameCallback = queryFileName;

  PRJ_NOTIFICATION_MAPPING notificationMappings[] = {
      {PRJ_NOTIFY_NEW_FILE_CREATED | PRJ_NOTIFY_FILE_OVERWRITTEN |
           PRJ_NOTIFY_PRE_RENAME | PRJ_NOTIFY_FILE_RENAMED |
           PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_MODIFIED |
           PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_DELETED |
           PRJ_NOTIFY_PRE_SET_HARDLINK,
       L""},
  };

  auto startOpts = PRJ_STARTVIRTUALIZING_OPTIONS();
  startOpts.NotificationMappings = notificationMappings;
  startOpts.NotificationMappingsCount =
      folly::to_narrow(std::size(notificationMappings));

  useNegativePathCaching_ = useNegativePathCaching;
  if (useNegativePathCaching) {
    startOpts.Flags = PRJ_FLAG_USE_NEGATIVE_PATH_CACHE;
  }

  XLOG(INFO) << "Starting PrjfsChannel for: " << mountPath_;

  auto winPath = mountPath_.wide();

  auto result = PrjMarkDirectoryAsPlaceholder(
      winPath.c_str(), nullptr, nullptr, mountId_);

  if (FAILED(result) &&
      result != HRESULT_FROM_WIN32(ERROR_REPARSE_POINT_ENCOUNTERED)) {
    throw makeHResultErrorExplicit(
        result, sformat("Failed to setup the mount point: {}", mountPath_));
  }

  result = PrjStartVirtualizing(
      winPath.c_str(), &callbacks, this, &startOpts, &mountChannel_);

  if (FAILED(result)) {
    throw makeHResultErrorExplicit(result, "Failed to start the mount point");
  }

  XLOG(INFO) << "Started PrjfsChannel for: " << mountPath_;

  isRunning_ = true;
}

void PrjfsChannel::stop() {
  XLOG(INFO) << sformat("Stopping PrjfsChannel for {}", mountPath_);
  DCHECK(isRunning_);
  PrjStopVirtualizing(mountChannel_);
  stopPromise_.setValue(FsChannel::StopData{});
  isRunning_ = false;
  mountChannel_ = nullptr;
}

folly::SemiFuture<FsChannel::StopData> PrjfsChannel::getStopFuture() {
  return stopPromise_.getSemiFuture();
}

// TODO: We need to add an extra layer to absorb all the exceptions generated in
// Eden from leaking into FS. This would come in soon.

void PrjfsChannel::removeCachedFile(RelativePathPiece path) {
  auto winPath = path.wide();

  XLOG(DBG6) << "Invalidating: " << path;

  PRJ_UPDATE_FAILURE_CAUSES failureReason;
  auto result = PrjDeleteFile(
      mountChannel_,
      winPath.c_str(),
      PRJ_UPDATE_ALLOW_DIRTY_METADATA | PRJ_UPDATE_ALLOW_DIRTY_DATA |
          PRJ_UPDATE_ALLOW_READ_ONLY | PRJ_UPDATE_ALLOW_TOMBSTONE,
      &failureReason);
  if (FAILED(result)) {
    XLOGF(
        DBG6,
        "Failed to delete disk file {}, reason: {}, error: {:x}",
        path,
        failureReason,
        static_cast<uint32_t>(result));
    // We aren't maintainting the information about which files were created
    // by the user vs through Eden backing store. The Projected FS will not
    // create tombstones when the user created files are renamed or deleted.
    // Until we have that information we cannot throw an exception on failure
    // here.
  }
}

void PrjfsChannel::addDirectoryPlaceholder(RelativePathPiece path) {
  auto winMountPath = mountPath_.wide();
  auto fullPath = mountPath_ + path;
  auto winPath = fullPath.wide();

  XLOGF(DBG6, "Adding a placeholder for: ", path);
  auto result = PrjMarkDirectoryAsPlaceholder(
      winMountPath.c_str(), winPath.c_str(), nullptr, mountId_);
  if (FAILED(result)) {
    XLOGF(
        DBG6,
        "Can't add a placeholder for {}: {:x}",
        path,
        static_cast<uint32_t>(result));
  }
}

void PrjfsChannel::flushNegativePathCache() {
  if (useNegativePathCaching_) {
    XLOG(DBG6) << "Flushing negative path cache";

    uint32_t numFlushed = 0;
    auto result = PrjClearNegativePathCache(mountChannel_, &numFlushed);
    if (FAILED(result)) {
      throwHResultErrorExplicit(
          result, "Couldn't flush the negative path cache");
    }

    XLOGF(DBG6, "Flushed {} entries", numFlushed);
  }
}

} // namespace eden
} // namespace facebook
