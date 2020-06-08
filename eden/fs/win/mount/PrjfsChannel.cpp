/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/mount/PrjfsChannel.h"
#include <folly/logging/xlog.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/win/mount/EdenDispatcher.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/StringConv.h"
#include "eden/fs/win/utils/WinError.h"

using folly::sformat;

namespace {

using facebook::eden::EdenDispatcher;

static EdenDispatcher* getDispatcher(
    const PRJ_CALLBACK_DATA* callbackData) noexcept {
  DCHECK(callbackData);
  auto dispatcher = static_cast<EdenDispatcher*>(callbackData->InstanceContext);
  DCHECK(dispatcher);
  DCHECK(dispatcher->isValidDispatcher());
  return dispatcher;
}

static HRESULT startEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  return getDispatcher(callbackData)
      ->startEnumeration(*callbackData, *enumerationId);
}

static HRESULT endEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  getDispatcher(callbackData)->endEnumeration(*enumerationId);
  return S_OK;
}

static HRESULT getEnumerationData(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId,
    PCWSTR searchExpression,
    PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) noexcept {
  return getDispatcher(callbackData)
      ->getEnumerationData(
          *callbackData,
          *enumerationId,
          searchExpression,
          dirEntryBufferHandle);
}

static HRESULT getPlaceholderInfo(
    const PRJ_CALLBACK_DATA* callbackData) noexcept {
  return getDispatcher(callbackData)->getFileInfo(*callbackData);
}

static HRESULT queryFileName(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  return getDispatcher(callbackData)->queryFileName(*callbackData);
}

static HRESULT getFileData(
    const PRJ_CALLBACK_DATA* callbackData,
    UINT64 byteOffset,
    UINT32 length) noexcept {
  return getDispatcher(callbackData)
      ->getFileData(*callbackData, byteOffset, length);
}

static HRESULT notification(
    const PRJ_CALLBACK_DATA* callbackData,
    BOOLEAN isDirectory,
    PRJ_NOTIFICATION notificationType,
    PCWSTR destinationFileName,
    PRJ_NOTIFICATION_PARAMETERS* notificationParameters) noexcept {
  getDispatcher(callbackData)
      ->notification(
          *callbackData,
          isDirectory,
          notificationType,
          destinationFileName,
          *notificationParameters);
  return S_OK;
}
} // namespace

namespace facebook {
namespace eden {

PrjfsChannel::PrjfsChannel(EdenMount* mount)
    : root_{mount->getPath()}, dispatcher_{*mount}, mountId_{Guid::generate()} {
  XLOG(INFO) << sformat(
      "Creating PrjfsChannel, mount ({}), MountPath ({})", mount, root_);

  // Setup mount root folder
  auto winPath = edenToWinPath(root_.stringPiece());
  HRESULT result = PrjMarkDirectoryAsPlaceholder(
      winPath.c_str(), nullptr, nullptr, mountId_);

  if (FAILED(result) &&
      result != HRESULT_FROM_WIN32(ERROR_REPARSE_POINT_ENCOUNTERED)) {
    throw makeHResultErrorExplicit(
        result, sformat("Failed to setup the mount point({})", root_));
  }
}

PrjfsChannel::~PrjfsChannel() {
  if (isRunning_) {
    stop();
  }
}

void PrjfsChannel::start() {
  auto callbacks = PRJ_CALLBACKS();
  auto options = PRJ_STARTVIRTUALIZING_OPTIONS();
  callbacks.StartDirectoryEnumerationCallback = startEnumeration;
  callbacks.EndDirectoryEnumerationCallback = endEnumeration;
  callbacks.GetDirectoryEnumerationCallback = getEnumerationData;
  callbacks.GetPlaceholderInfoCallback = getPlaceholderInfo;
  callbacks.GetFileDataCallback = getFileData;
  callbacks.NotificationCallback = notification;
  callbacks.QueryFileNameCallback = queryFileName;

  PRJ_NOTIFICATION_MAPPING notificationMappings[] = {
      {PRJ_NOTIFY_NEW_FILE_CREATED | PRJ_NOTIFY_FILE_OVERWRITTEN |
           PRJ_NOTIFY_FILE_RENAMED |
           PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_MODIFIED |
           PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_DELETED,
       L""},
      {PRJ_NOTIFY_SUPPRESS_NOTIFICATIONS, L".hg"},
      {PRJ_NOTIFY_SUPPRESS_NOTIFICATIONS, L".eden"},
  };

  PRJ_STARTVIRTUALIZING_OPTIONS startOpts = {};
  startOpts.NotificationMappings = notificationMappings;
  startOpts.NotificationMappingsCount =
      folly::to_narrow(std::size(notificationMappings));

  auto dispatcher = getDispatcher();
  XLOG(INFO) << sformat(
      "Starting PrjfsChannel Path ({}) Dispatcher (0x{:x})",
      root_,
      uintptr_t(dispatcher));
  DCHECK(dispatcher->isValidDispatcher());

  auto winPath = edenToWinPath(root_.stringPiece());
  HRESULT result = PrjStartVirtualizing(
      winPath.c_str(), &callbacks, dispatcher, &startOpts, &mountChannel_);

  if (FAILED(result)) {
    throw makeHResultErrorExplicit(result, "Failed to start the mount point");
  }

  isRunning_ = true;
}

void PrjfsChannel::stop() {
  XLOG(INFO) << sformat("Stopping PrjfsChannel ({})", root_);
  DCHECK(isRunning_);
  PrjStopVirtualizing(mountChannel_);
  isRunning_ = false;
  mountChannel_ = nullptr;
}

// TODO: We need to add an extra layer to absorb all the exceptions generated in
// Eden from leaking into FS. This would come in soon.

void PrjfsChannel::deleteFile(
    RelativePathPiece path,
    PRJ_UPDATE_TYPES updateFlags) {
  auto winPath = edenToWinPath((root_ + path).stringPiece());
  PRJ_UPDATE_FAILURE_CAUSES failureReason;
  HRESULT hr = PrjDeleteFile(
      mountChannel_, winPath.c_str(), updateFlags, &failureReason);
  if (hr != S_OK) {
    XLOGF(
        DBG6,
        "Failed to delete disk file {} reason: {} error: {}",
        path,
        static_cast<uint32_t>(failureReason),
        hr);
    // We aren't maintainting the information about which files were created
    // by the user vs through Eden backing store. The Projected FS will not
    // create tombstones when the user created files are renamed or deleted.
    // Until we have that information we cannot throw an exception on failure
    // here.
  }
}

void PrjfsChannel::removeCachedFile(RelativePathPiece path) {
  deleteFile(
      path,
      PRJ_UPDATE_ALLOW_DIRTY_METADATA | PRJ_UPDATE_ALLOW_DIRTY_DATA |
          PRJ_UPDATE_ALLOW_READ_ONLY | PRJ_UPDATE_ALLOW_TOMBSTONE);
}

void PrjfsChannel::removeDeletedFile(RelativePathPiece path) {
  deleteFile(path, PRJ_UPDATE_ALLOW_TOMBSTONE);
}

} // namespace eden
} // namespace facebook
