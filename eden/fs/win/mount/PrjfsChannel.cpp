/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "PrjfsChannel.h"
#include "folly/portability/Windows.h"

#include <folly/logging/xlog.h>
#include <string>
#include "eden/fs/win/mount/EdenDispatcher.h"
#include "eden/fs/win/mount/EdenMount.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/StringConv.h"
#include "eden/fs/win/utils/WinError.h"

using folly::sformat;

namespace facebook {
namespace eden {

PrjfsChannel::PrjfsChannel(EdenMount* mount)
    : mount_{mount},
      mountId_{Guid::generate()},
      winPath_{edenToWinPath(mount->getPath().value())} {
  XLOG(INFO) << sformat(
      "Creating PrjfsChannel, mount ({}), MountPath ({})",
      mount,
      mount->getPath());

  //
  // The root will be created by the cli before calling mount. Make sure it
  // is created else create it.
  //
  if (!CreateDirectoryW(winPath_.c_str(), nullptr)) {
    DWORD error = GetLastError();
    if (error != ERROR_ALREADY_EXISTS) {
      throw makeWin32ErrorExplicit(
          error,
          sformat("Failed to create the mount point ({})", mount->getPath()));
    }
  } else {
    XLOG(INFO) << sformat(
        "Mount point did not exist created new ({}), MountPath ({})",
        mount,
        mount->getPath());
  }

  // Setup mount root folder
  HRESULT result = PrjMarkDirectoryAsPlaceholder(
      winPath_.c_str(), nullptr, nullptr, mountId_);

  if (FAILED(result) &&
      result != HRESULT_FROM_WIN32(ERROR_REPARSE_POINT_ENCOUNTERED)) {
    throw makeHResultErrorExplicit(
        result,
        sformat("Failed to setup the mount point({})", mount->getPath()));
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

  PRJ_STARTVIRTUALIZING_OPTIONS startOpts = {};
  PRJ_NOTIFICATION_MAPPING notificationMappings[3] = {};

  notificationMappings[0].NotificationRoot = L"";
  notificationMappings[0].NotificationBitMask = PRJ_NOTIFY_NEW_FILE_CREATED |
      PRJ_NOTIFY_FILE_OVERWRITTEN | PRJ_NOTIFY_FILE_RENAMED |
      PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_MODIFIED |
      PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_DELETED;

  notificationMappings[1].NotificationRoot = L".hg";
  notificationMappings[1].NotificationBitMask =
      PRJ_NOTIFY_SUPPRESS_NOTIFICATIONS;

  notificationMappings[2].NotificationRoot = L".eden";
  notificationMappings[2].NotificationBitMask =
      PRJ_NOTIFY_SUPPRESS_NOTIFICATIONS;

  startOpts.NotificationMappings = notificationMappings;
  startOpts.NotificationMappingsCount = std::size(notificationMappings);

  auto dispatcher = mount_->getDispatcher();
  XLOG(INFO) << sformat(
      "Starting PrjfsChannel Path ({}) Dispatcher (0x{:x})",
      mount_->getPath(),
      uintptr_t(dispatcher));
  DCHECK(dispatcher->isValidDispatcher());

  HRESULT result = PrjStartVirtualizing(
      winPath_.c_str(), &callbacks, dispatcher, &startOpts, &mountChannel_);

  if (FAILED(result)) {
    throw makeHResultErrorExplicit(result, "Failed to start the mount point");
  }

  isRunning_ = true;
}

void PrjfsChannel::stop() {
  XLOG(INFO) << sformat("Stopping PrjfsChannel ({})", mount_->getPath());
  DCHECK(isRunning_);
  PrjStopVirtualizing(mountChannel_);
  isRunning_ = false;
  mountChannel_ = nullptr;
}

// TODO: We need to add an extra layer to absorb all the exceptions generated in
// Eden from leaking into FS. This would come in soon.

EdenDispatcher* PrjfsChannel::getDispatcher(
    const PRJ_CALLBACK_DATA* callbackData) noexcept {
  DCHECK(callbackData);
  auto dispatcher = static_cast<EdenDispatcher*>(callbackData->InstanceContext);
  DCHECK(dispatcher);
  DCHECK(dispatcher->isValidDispatcher());
  return dispatcher;
}

HRESULT PrjfsChannel::startEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  return getDispatcher(callbackData)
      ->startEnumeration(*callbackData, *enumerationId);
}

HRESULT PrjfsChannel::endEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  getDispatcher(callbackData)->endEnumeration(*enumerationId);
  return S_OK;
}

HRESULT PrjfsChannel::getEnumerationData(
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

HRESULT PrjfsChannel::getPlaceholderInfo(
    const PRJ_CALLBACK_DATA* callbackData) noexcept {
  return getDispatcher(callbackData)->getFileInfo(*callbackData);
}

HRESULT PrjfsChannel::getFileData(
    const PRJ_CALLBACK_DATA* callbackData,
    UINT64 byteOffset,
    UINT32 length) noexcept {
  return getDispatcher(callbackData)
      ->getFileData(*callbackData, byteOffset, length);
}

HRESULT PrjfsChannel::notification(
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

} // namespace eden
} // namespace facebook
