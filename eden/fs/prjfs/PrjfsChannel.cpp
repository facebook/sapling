/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/prjfs/PrjfsChannel.h"
#include <fmt/format.h>
#include <folly/logging/xlog.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/prjfs/Dispatcher.h"
#include "eden/fs/prjfs/PrjfsRequestContext.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/NotImplemented.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/WinError.h"

namespace {

using facebook::eden::ChannelThreadStats;
using facebook::eden::Dispatcher;
using facebook::eden::exceptionToHResult;
using facebook::eden::Guid;
using facebook::eden::InodeMetadata;
using facebook::eden::makeHResultErrorExplicit;
using facebook::eden::ObjectFetchContext;
using facebook::eden::PrjfsChannel;
using facebook::eden::PrjfsRequestContext;
using facebook::eden::RelativePath;
using facebook::eden::RelativePathPiece;
using facebook::eden::RequestMetricsScope;
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
    auto channel = getChannel(callbackData);
    auto dispatcher = channel->getDispatcher();
    auto guid = Guid(*enumerationId);
    auto context =
        std::make_unique<PrjfsRequestContext>(channel, *callbackData);
    auto path = RelativePath(callbackData->FilePathName);

    context
        ->catchErrors(folly::makeFutureWith([context = context.get(),
                                             dispatcher = dispatcher,
                                             guid = std::move(guid),
                                             path = std::move(path)] {
          auto requestWatch =
              std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(
                  nullptr);
          auto histogram = &ChannelThreadStats::openDir;
          context->startRequest(
              dispatcher->getStats(), histogram, requestWatch);

          return dispatcher->opendir(path, std::move(guid), *context)
              .thenValue(
                  [context = context](auto&&) { context->sendSuccess(); });
        }))
        .ensure([context = std::move(context)] {});

    return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

HRESULT endEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);

  try {
    auto guid = Guid(*enumerationId);

    getChannel(callbackData)->getDispatcher()->closedir(guid);

    return S_OK;
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
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
    auto channel = getChannel(callbackData);
    auto dispatcher = channel->getDispatcher();
    auto context =
        std::make_unique<PrjfsRequestContext>(channel, *callbackData);

    auto path = RelativePath(callbackData->FilePathName);
    auto virtualizationContext = callbackData->NamespaceVirtualizationContext;

    context
        ->catchErrors(folly::makeFutureWith([context = context.get(),
                                             dispatcher = dispatcher,
                                             path = std::move(path),
                                             virtualizationContext =
                                                 virtualizationContext] {
          auto requestWatch =
              std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(
                  nullptr);
          auto histogram = &ChannelThreadStats::lookup;
          context->startRequest(
              dispatcher->getStats(), histogram, requestWatch);

          return dispatcher->lookup(std::move(path), *context)
              .thenValue([context = context,
                          virtualizationContext = virtualizationContext](
                             const std::optional<InodeMetadata>&& optMetadata) {
                if (!optMetadata) {
                  context->sendError(HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND));
                  return folly::makeFuture(folly::unit);
                }
                auto metadata = std::move(optMetadata).value();

                PRJ_PLACEHOLDER_INFO placeholderInfo{};
                placeholderInfo.FileBasicInfo.IsDirectory = metadata.isDir;
                placeholderInfo.FileBasicInfo.FileSize = metadata.size;
                auto inodeName = metadata.path.wide();

                HRESULT result = PrjWritePlaceholderInfo(
                    virtualizationContext,
                    inodeName.c_str(),
                    &placeholderInfo,
                    sizeof(placeholderInfo));

                if (FAILED(result)) {
                  return folly::makeFuture<folly::Unit>(
                      makeHResultErrorExplicit(
                          result,
                          fmt::format(
                              FMT_STRING("Writing placeholder for {}"),
                              metadata.path)));
                }

                context->sendSuccess();
                return folly::makeFuture(folly::unit);
              });
        }))
        .ensure([context = std::move(context)] {});

    return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

HRESULT queryFileName(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);

  try {
    auto channel = getChannel(callbackData);
    auto dispatcher = channel->getDispatcher();
    auto context =
        std::make_unique<PrjfsRequestContext>(channel, *callbackData);

    auto path = RelativePath(callbackData->FilePathName);

    context
        ->catchErrors(folly::makeFutureWith([context = context.get(),
                                             dispatcher = dispatcher,
                                             path = std::move(path)] {
          auto requestWatch =
              std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(
                  nullptr);
          auto histogram = &ChannelThreadStats::access;
          context->startRequest(
              dispatcher->getStats(), histogram, requestWatch);

          return dispatcher->access(std::move(path), *context)
              .thenValue([context = context](bool present) {
                if (present) {
                  context->sendSuccess();
                } else {
                  context->sendError(HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND));
                }
              });
        }))
        .ensure([context = std::move(context)] {});

    return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

struct PrjAlignedBufferDeleter {
  void operator()(void* buffer) noexcept {
    ::PrjFreeAlignedBuffer(buffer);
  }
};

HRESULT readMultipleFileChunks(
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
    const Guid& dataStreamId,
    const std::string& content,
    uint64_t startOffset,
    uint64_t length,
    uint64_t chunkSize) {
  HRESULT result;
  std::unique_ptr<void, PrjAlignedBufferDeleter> writeBuffer{
      PrjAllocateAlignedBuffer(namespaceVirtualizationContext, chunkSize)};

  if (writeBuffer.get() == nullptr) {
    return E_OUTOFMEMORY;
  }

  uint64_t remainingLength = length;

  while (remainingLength > 0) {
    uint64_t copySize = std::min(remainingLength, chunkSize);

    //
    // TODO(puneetk): Once backing store has the support for chunking the file
    // contents, we can read the chunks of large files here and then write
    // them to FS.
    //
    // TODO(puneetk): Build an interface to backing store so that we can pass
    // the aligned buffer to avoid coping here.
    //
    RtlCopyMemory(writeBuffer.get(), content.data() + startOffset, copySize);

    // Write the data to the file in the local file system.
    result = PrjWriteFileData(
        namespaceVirtualizationContext,
        dataStreamId,
        writeBuffer.get(),
        startOffset,
        folly::to_narrow(copySize));

    if (FAILED(result)) {
      return result;
    }

    remainingLength -= copySize;
    startOffset += copySize;
  }

  return S_OK;
}

HRESULT readSingleFileChunk(
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
    const Guid& dataStreamId,
    const std::string& content,
    uint64_t startOffset,
    uint64_t length) {
  return readMultipleFileChunks(
      namespaceVirtualizationContext,
      dataStreamId,
      content,
      /*startOffset=*/startOffset,
      /*length=*/length,
      /*writeLength=*/length);
}

uint64_t BlockAlignTruncate(uint64_t ptr, uint32_t alignment) {
  return ((ptr) & (0 - (static_cast<uint64_t>(alignment))));
}

constexpr uint32_t kMinChunkSize = 512 * 1024; // 512 KiB
constexpr uint32_t kMaxChunkSize = 5 * 1024 * 1024; // 5 MiB

HRESULT getFileData(
    const PRJ_CALLBACK_DATA* callbackData,
    UINT64 byteOffset,
    UINT32 length) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);

  try {
    auto channel = getChannel(callbackData);
    auto dispatcher = channel->getDispatcher();
    auto context =
        std::make_unique<PrjfsRequestContext>(channel, *callbackData);

    context
        ->catchErrors(folly::makeFutureWith(
            [context = context.get(),
             dispatcher = dispatcher,
             path = RelativePath(callbackData->FilePathName),
             virtualizationContext =
                 callbackData->NamespaceVirtualizationContext,
             dataStreamId = Guid(callbackData->DataStreamId),
             byteOffset = byteOffset,
             length = length] {
              auto requestWatch =
                  std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(
                      nullptr);
              auto histogram = &ChannelThreadStats::read;
              context->startRequest(
                  dispatcher->getStats(), histogram, requestWatch);

              return dispatcher
                  ->read(std::move(path), byteOffset, length, *context)
                  .thenValue([context = context,
                              virtualizationContext = virtualizationContext,
                              dataStreamId = std::move(dataStreamId),
                              byteOffset = byteOffset,
                              length = length](const std::string content) {
                    //
                    // We should return file data which is smaller than
                    // our kMaxChunkSize and meets the memory alignment
                    // requirements of the virtualization instance's storage
                    // device.
                    //

                    HRESULT result;
                    if (content.length() <= kMinChunkSize) {
                      //
                      // If the file is small - copy the whole file in one shot.
                      //
                      result = readSingleFileChunk(
                          virtualizationContext,
                          dataStreamId,
                          content,
                          /*startOffset=*/0,
                          /*writeLength=*/content.length());

                    } else if (length <= kMaxChunkSize) {
                      //
                      // If the request is with in our kMaxChunkSize - copy the
                      // entire request.
                      //
                      result = readSingleFileChunk(
                          virtualizationContext,
                          dataStreamId,
                          content,
                          /*startOffset=*/byteOffset,
                          /*writeLength=*/length);
                    } else {
                      //
                      // When the request is larger than kMaxChunkSize we split
                      // the request into multiple chunks.
                      //
                      PRJ_VIRTUALIZATION_INSTANCE_INFO instanceInfo;
                      result = PrjGetVirtualizationInstanceInfo(
                          virtualizationContext, &instanceInfo);

                      if (SUCCEEDED(result)) {
                        uint64_t startOffset = byteOffset;
                        uint64_t endOffset = BlockAlignTruncate(
                            startOffset + kMaxChunkSize,
                            instanceInfo.WriteAlignment);
                        DCHECK(endOffset > 0);
                        DCHECK(endOffset > startOffset);

                        uint64_t chunkSize = endOffset - startOffset;
                        result = readMultipleFileChunks(
                            virtualizationContext,
                            dataStreamId,
                            content,
                            /*startOffset=*/startOffset,
                            /*length=*/length,
                            /*chunkSize=*/chunkSize);
                      }
                    }

                    if (FAILED(result)) {
                      context->sendError(result);
                    } else {
                      context->sendSuccess();
                    }
                  });
            }))
        .ensure([context = std::move(context)] {});

    return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

void cancelCommand(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  // TODO(T67329233): Interrupt the future.
}

typedef folly::Future<folly::Unit> (Dispatcher::*NotificationHandler)(
    RelativePathPiece oldPath,
    RelativePathPiece destPath,
    bool isDirectory,
    ObjectFetchContext& context);

struct NotificationHandlerEntry {
  constexpr NotificationHandlerEntry() = default;
  constexpr NotificationHandlerEntry(
      NotificationHandler h,
      ChannelThreadStats::HistogramPtr hist)
      : handler{h}, histogram{hist} {}

  NotificationHandler handler = nullptr;
  ChannelThreadStats::HistogramPtr histogram = nullptr;
};

const std::unordered_map<PRJ_NOTIFICATION, NotificationHandlerEntry>
    notificationHandlerMap = {
        {
            PRJ_NOTIFICATION_NEW_FILE_CREATED,
            {&Dispatcher::newFileCreated, &ChannelThreadStats::newFileCreated},
        },
        {
            PRJ_NOTIFICATION_FILE_OVERWRITTEN,
            {&Dispatcher::fileOverwritten,
             &ChannelThreadStats::fileOverwritten},
        },
        {
            PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_MODIFIED,
            {&Dispatcher::fileHandleClosedFileModified,
             &ChannelThreadStats::fileHandleClosedFileModified},
        },
        {
            PRJ_NOTIFICATION_FILE_RENAMED,
            {&Dispatcher::fileRenamed, &ChannelThreadStats::fileRenamed},
        },
        {
            PRJ_NOTIFICATION_PRE_RENAME,
            {&Dispatcher::preRename, &ChannelThreadStats::preRenamed},
        },
        {
            PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_DELETED,
            {&Dispatcher::fileHandleClosedFileDeleted,
             &ChannelThreadStats::fileHandleClosedFileDeleted},
        },
        {
            PRJ_NOTIFICATION_PRE_SET_HARDLINK,
            {&Dispatcher::preSetHardlink, &ChannelThreadStats::preSetHardlink},
        },
};

HRESULT notification(
    const PRJ_CALLBACK_DATA* callbackData,
    BOOLEAN isDirectory,
    PRJ_NOTIFICATION notificationType,
    PCWSTR destinationFileName,
    PRJ_NOTIFICATION_PARAMETERS* notificationParameters) noexcept {
  BAIL_ON_RECURSIVE_CALL(callbackData);

  try {
    auto it = notificationHandlerMap.find(notificationType);
    if (it == notificationHandlerMap.end()) {
      XLOG(WARN) << "Unrecognized notification: " << notificationType;
      return HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER);
    } else {
      auto channel = getChannel(callbackData);
      auto dispatcher = channel->getDispatcher();
      auto context =
          std::make_unique<PrjfsRequestContext>(channel, *callbackData);
      auto histogram = it->second.histogram;
      auto handler = it->second.handler;

      auto relPath = RelativePath(callbackData->FilePathName);
      auto destPath = RelativePath(destinationFileName);

      context
          ->catchErrors(folly::makeFutureWith([context = context.get(),
                                               handler = handler,
                                               histogram = histogram,
                                               dispatcher = dispatcher,
                                               relPath = std::move(relPath),
                                               destPath = std::move(destPath),
                                               isDirectory] {
            auto requestWatch =
                std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(
                    nullptr);
            context->startRequest(
                dispatcher->getStats(), histogram, requestWatch);

            return (dispatcher->*handler)(
                       relPath, destPath, isDirectory, *context)
                .thenValue([context = context](auto&&) {
                  context->sendNotificationSuccess();
                });
          }))
          // Make sure that the context is alive for the duration of the future.
          .ensure([context = std::move(context)] {});

      return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
    }
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

} // namespace

namespace facebook {
namespace eden {

PrjfsChannel::PrjfsChannel(
    AbsolutePathPiece mountPath,
    Dispatcher* const dispatcher,
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
  callbacks.CancelCommandCallback = cancelCommand;

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
        result,
        fmt::format(
            FMT_STRING("Failed to setup the mount point: {}"), mountPath_));
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
  XLOG(INFO) << "Stopping PrjfsChannel for: " << mountPath_;
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

namespace {
void sendReply(
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT context,
    int32_t commandId,
    HRESULT result,
    PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS* FOLLY_NULLABLE extra) {
  result = PrjCompleteCommand(context, commandId, result, extra);
  if (FAILED(result)) {
    XLOG(ERR) << "Couldn't complete command: " << commandId << ": "
              << win32ErrorToString(result);
  }
}
} // namespace

void PrjfsChannel::sendSuccess(
    int32_t commandId,
    PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS* FOLLY_NULLABLE extra) {
  sendReply(getMountChannelContext(), commandId, S_OK, extra);
}

void PrjfsChannel::sendError(int32_t commandId, HRESULT result) {
  sendReply(getMountChannelContext(), commandId, result, nullptr);
}

} // namespace eden
} // namespace facebook
