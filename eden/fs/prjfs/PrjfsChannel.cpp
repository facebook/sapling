/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/prjfs/PrjfsChannel.h"
#include <fmt/format.h>
#include <folly/logging/xlog.h>

#include "eden/common/utils/StringConv.h"
#include "eden/common/utils/WinError.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/notifications/Notifier.h"
#include "eden/fs/prjfs/PrjfsDispatcher.h"
#include "eden/fs/prjfs/PrjfsRequestContext.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/NotImplemented.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/StaticAssert.h"

namespace facebook::eden {

using namespace std::literals::chrono_literals;

namespace {
// These static asserts exist to make explicit the memory usage of the per-mount
// PrjfsTraceBus. TraceBus uses 2 * capacity * sizeof(TraceEvent) memory usage,
// so limit total memory usage to around 1 MB per mount.
static_assert(CheckSize<PrjfsTraceEvent, 48>());

PPWPI2 placeholderExtendedInfo2_{nullptr};
PPFDEB2 prjFillDirEntryBuffer2_{nullptr};

// TODO: Remove once the build has switched to a more recent SDK
HRESULT PrjWritePlaceholderInfo2(
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
    PCWSTR destinationFileName,
    const PRJ_PLACEHOLDER_INFO* placeholderInfo,
    UINT32 placeholderInfoSize,
    const PRJ_EXTENDED_INFO* ExtendedInfo) {
  return placeholderExtendedInfo2_(
      namespaceVirtualizationContext,
      destinationFileName,
      placeholderInfo,
      placeholderInfoSize,
      ExtendedInfo);
}

HRESULT PrjFillDirEntryBuffer2(
    PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle,
    PCWSTR fileName,
    PRJ_FILE_BASIC_INFO* fileBasicInfo,
    PRJ_EXTENDED_INFO* extendedInfo) {
  return prjFillDirEntryBuffer2_(
      dirEntryBufferHandle, fileName, fileBasicInfo, extendedInfo);
}

folly::ReadMostlySharedPtr<PrjfsChannelInner> getChannel(
    const PRJ_CALLBACK_DATA* callbackData) noexcept {
  XDCHECK(callbackData);
  auto* channel = static_cast<PrjfsChannel*>(callbackData->InstanceContext);
  XDCHECK(channel);
  return channel->getInner();
}

/**
 * ProjectedFS gives us a full device path for the application that triggered
 * the IO, this trims it an returns a view onto the last component.
 *
 * The lifetime of the returned view is the same as the lifetime of the
 * argument.
 */
std::wstring_view basenameFromAppName(PCWSTR fullAppName) noexcept {
  auto fullAppNameView = std::wstring_view{fullAppName};
  auto lastSlash = fullAppNameView.find_last_of(L'\\');
  return fullAppNameView.substr(lastSlash + 1);
}

/**
 * Disallow some know applications that force EdenFS to overfetch files.
 *
 * Some backup applications or indexing are ignoring the
 * FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS attribute attached to all EdenFS
 * files/directories and are therefore forcing the entire repository to be
 * fetched. Since this isn't the intention of these applications, simply
 * disallow them from accessing anything on EdenFS.
 */
bool disallowMisbehavingApplications(PCWSTR fullAppName) noexcept {
  if (fullAppName == nullptr) {
    return false;
  }

  constexpr std::wstring_view misbehavingApps[] = {
      L"Code42Service.exe",
      L"CrashPlanService.exe",
      L"windirstat.exe",
      L"AgentRansack.exe",
  };

  auto appName = basenameFromAppName(fullAppName);
  for (auto misbehavingApp : misbehavingApps) {
    if (appName == misbehavingApp) {
      XLOG(DBG6) << "Stopping \"" << wideToMultibyteString<std::string>(appName)
                 << "\" from accessing the repository.";
      return true;
    }
  }

  return false;
}
} // namespace

namespace detail {
struct PrjfsLiveRequest {
  PrjfsLiveRequest(
      std::shared_ptr<TraceBus<PrjfsTraceEvent>> traceBus,
      const std::atomic<size_t>& traceDetailedArguments,
      PrjfsTraceCallType callType,
      const PRJ_CALLBACK_DATA& data,
      LPCWSTR destinationFileName)
      : traceBus_{std::move(traceBus)}, type_{callType}, data_{data} {
    if (traceDetailedArguments.load(std::memory_order_acquire)) {
      traceBus_->publish(PrjfsTraceEvent::start(
          callType, data_, formatTraceEventString(data, destinationFileName)));
    } else {
      traceBus_->publish(PrjfsTraceEvent::start(callType, data_));
    }
  }

  PrjfsLiveRequest(PrjfsLiveRequest&& that) noexcept = default;
  PrjfsLiveRequest& operator=(PrjfsLiveRequest&&) = delete;

  ~PrjfsLiveRequest() {
    if (traceBus_) {
      traceBus_->publish(PrjfsTraceEvent::finish(type_, data_));
    }
  }

  std::string formatTraceEventString(
      const PRJ_CALLBACK_DATA& data,
      LPCWSTR destinationFileName) {
    // Most events only have data.FilePathName set to a repo-relative path,
    // describing the file that is related to the event.
    //
    // This path can be the empty string L"" if the operation is in the repo
    // root directory, such as `dir %REPO_ROOT%`. In these cases,
    // destinationFileName is nullptr, either pass explicitly in this codebase,
    // or given to the ::notification() function (which is implmentation of
    // PRJ_NOTIFICATION_CB).
    //
    // Some operations have both a src and destination path, like *RENAME or
    // *SET_HARDLINK. In these cases, destinationFileName may be a pointer to a
    // string. This string is zero-length if the destination file in question is
    // outside the repo. To make this more readable in the logs, if
    // destinationFileName is provided (non-nullptr), we convert zero-length
    // paths to `nonRepoPath` below. This conversion is not done when
    // destinationFileName is nullptr, because we don't want to falsely
    // represent other operations on the repo root as operating on a non-repo
    // path.
    static const wchar_t nonRepoPath[] = L"<non-repo-path>";
    LPCWSTR relativeFileName = data.FilePathName;
    if (destinationFileName != nullptr) {
      if (relativeFileName && !relativeFileName[0]) {
        relativeFileName = nonRepoPath;
      }
      if (destinationFileName && !destinationFileName[0]) {
        destinationFileName = nonRepoPath;
      }
    }

    return fmt::format(
        "{} from {}({}): {}({}{}{})",
        data_.commandId,
        processPathToName(data.TriggeringProcessImageFileName),
        data_.pid,
        apache::thrift::util::enumName(type_, "(unknown)"),
        relativeFileName == nullptr ? RelativePath{}
                                    : RelativePath(relativeFileName),
        (destinationFileName && relativeFileName) ? "=>" : "",
        destinationFileName == nullptr ? RelativePath{}
                                       : RelativePath(destinationFileName));
  }

 private:
  std::string processPathToName(PCWSTR fullAppName) {
    if (fullAppName == nullptr) {
      return "None";
    } else {
      auto appName = basenameFromAppName(fullAppName);
      return wideToMultibyteString<std::string>(appName);
    }
  }

  std::shared_ptr<TraceBus<PrjfsTraceEvent>> traceBus_;
  PrjfsTraceCallType type_;
  PrjfsTraceEvent::PrjfsOperationData data_;
};
} // namespace detail

namespace {

template <class Method, class... Args>
HRESULT runCallback(
    Method method,
    PrjfsTraceCallType callType,
    const PRJ_CALLBACK_DATA* callbackData,
    LPCWSTR destinationFileName,
    Args&&... args) noexcept {
  try {
    if (disallowMisbehavingApplications(
            callbackData->TriggeringProcessImageFileName)) {
      return HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);
    }

    auto channel = getChannel(callbackData);
    if (!channel) {
      return HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER);
    }

    auto channelPtr = channel.get();
    auto context = std::make_shared<PrjfsRequestContext>(
        std::move(channel), *callbackData);
    auto liveRequest = std::make_unique<detail::PrjfsLiveRequest>(
        channelPtr->getTraceBusPtr(),
        channelPtr->getTraceDetailedArguments(),
        callType,
        *callbackData,
        destinationFileName);
    return (channelPtr->*method)(
        std::move(context),
        callbackData,
        std::move(liveRequest),
        std::forward<Args>(args)...);
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

/**
 * Log on callbacks triggered by EdenFS.
 *
 * All callbacks besides the "notification" one are allowed to be called from
 * EdenFS itself, this is due to these only accessing data from the
 * ObjectStore which will never perform any disk IO to the working copy. To
 * handle out of order notifications about file/directory changes, the
 * "notification" callback may need to read the working copy, which may
 * trigger some callbacks to be triggered. These are OK due to the property
 * described above.
 */
void allowRecursiveCallbacks(const PRJ_CALLBACK_DATA* callbackData) {
  if (callbackData->TriggeringProcessId == GetCurrentProcessId()) {
    XLOG(DBG6) << "Recursive EdenFS call for: "
               << RelativePath(callbackData->FilePathName);
  }
}

HRESULT startEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  allowRecursiveCallbacks(callbackData);
  return runCallback(
      &PrjfsChannelInner::startEnumeration,
      PrjfsTraceCallType::START_ENUMERATION,
      callbackData,
      nullptr,
      enumerationId);
}

HRESULT endEnumeration(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId) noexcept {
  allowRecursiveCallbacks(callbackData);
  return runCallback(
      &PrjfsChannelInner::endEnumeration,
      PrjfsTraceCallType::END_ENUMERATION,
      callbackData,
      nullptr,
      enumerationId);
}

HRESULT getEnumerationData(
    const PRJ_CALLBACK_DATA* callbackData,
    const GUID* enumerationId,
    PCWSTR searchExpression,
    PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) noexcept {
  allowRecursiveCallbacks(callbackData);
  return runCallback(
      &PrjfsChannelInner::getEnumerationData,
      PrjfsTraceCallType::GET_ENUMERATION_DATA,
      callbackData,
      nullptr,
      enumerationId,
      searchExpression,
      dirEntryBufferHandle);
}

HRESULT getPlaceholderInfo(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  allowRecursiveCallbacks(callbackData);
  return runCallback(
      &PrjfsChannelInner::getPlaceholderInfo,
      PrjfsTraceCallType::GET_PLACEHOLDER_INFO,
      callbackData,
      nullptr);
}

HRESULT queryFileName(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  allowRecursiveCallbacks(callbackData);
  return runCallback(
      &PrjfsChannelInner::queryFileName,
      PrjfsTraceCallType::QUERY_FILE_NAME,
      callbackData,
      nullptr);
}

HRESULT getFileData(
    const PRJ_CALLBACK_DATA* callbackData,
    UINT64 byteOffset,
    UINT32 length) noexcept {
  allowRecursiveCallbacks(callbackData);
  return runCallback(
      &PrjfsChannelInner::getFileData,
      PrjfsTraceCallType::GET_FILE_DATA,
      callbackData,
      nullptr,
      byteOffset,
      length);
}

void cancelCommand(const PRJ_CALLBACK_DATA* callbackData) noexcept {
  allowRecursiveCallbacks(callbackData);
  // TODO(T67329233): Interrupt the future.
  XLOG(DBG6) << "Cancellation requested for command: "
             << callbackData->CommandId;
}

namespace {
const std::unordered_map<PRJ_NOTIFICATION, PrjfsTraceCallType>
    notificationTypeMap = {
        {PRJ_NOTIFICATION_NEW_FILE_CREATED,
         PrjfsTraceCallType::NEW_FILE_CREATED},
        {PRJ_NOTIFICATION_PRE_DELETE, PrjfsTraceCallType::PRE_DELETE},
        {PRJ_NOTIFICATION_FILE_OVERWRITTEN,
         PrjfsTraceCallType::FILE_OVERWRITTEN},
        {PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_MODIFIED,
         PrjfsTraceCallType::FILE_HANDLE_CLOSED_FILE_MODIFIED},
        {PRJ_NOTIFICATION_FILE_RENAMED, PrjfsTraceCallType::FILE_RENAMED},
        {PRJ_NOTIFICATION_PRE_RENAME, PrjfsTraceCallType::PRE_RENAME},
        {PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_DELETED,
         PrjfsTraceCallType::FILE_HANDLE_CLOSED_FILE_DELETED},
        {PRJ_NOTIFICATION_PRE_SET_HARDLINK,
         PrjfsTraceCallType::PRE_SET_HARDLINK},
        {PRJ_NOTIFICATION_FILE_PRE_CONVERT_TO_FULL,
         PrjfsTraceCallType::FILE_PRE_CONVERT_TO_FULL}};
} // namespace

HRESULT notification(
    const PRJ_CALLBACK_DATA* callbackData,
    BOOLEAN isDirectory,
    PRJ_NOTIFICATION notificationType,
    PCWSTR destinationFileName,
    PRJ_NOTIFICATION_PARAMETERS* notificationParameters) noexcept {
  try {
    auto channel = getChannel(callbackData);
    if (!channel) {
      // TODO(zeyi): Something modified the working copy while it is being
      // unmounted. At this point, we have no way to deal with this properly
      // and the next time this repository is mounted, there will be a
      // discrepency between what EdenFS thinks the state of the working copy
      // should be and what it actually is. To solve this, we will need to
      // scan the working copy at mount time to find these files and fixup
      // EdenFS inodes. Once the above is done, refactor this code to use
      // runCallback.
      EDEN_BUG() << "A notification was received while unmounting";
    }

    auto channelPtr = channel.get();
    auto context = std::make_shared<PrjfsRequestContext>(
        std::move(channel), *callbackData);
    auto typeIt = notificationTypeMap.find(notificationType);
    auto nType = PrjfsTraceCallType::INVALID;
    if (typeIt != notificationTypeMap.end()) {
      nType = typeIt->second;
    }
    auto liveRequest = detail::PrjfsLiveRequest{
        channelPtr->getTraceBusPtr(),
        channelPtr->getTraceDetailedArguments(),
        nType,
        *callbackData,
        destinationFileName};
    return channelPtr->notification(
        std::move(context),
        callbackData,
        isDirectory,
        notificationType,
        destinationFileName,
        notificationParameters);
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

/**
 * Detach the passed in future onto the global CPU executor.
 */
void detachAndCompleteCallback(
    ImmediateFuture<folly::Unit> future,
    std::shared_ptr<PrjfsRequestContext> context,
    std::unique_ptr<detail::PrjfsLiveRequest> liveRequest) {
  auto completionFuture =
      context->catchErrors(std::move(future))
          .ensure([context = std::move(context),
                   liveRequest = std::move(liveRequest)] {});
  if (!completionFuture.isReady()) {
    folly::futures::detachOnGlobalCPUExecutor(
        std::move(completionFuture).semi());
  }
}

} // namespace

PrjfsChannelInner::PrjfsChannelInner(
    std::unique_ptr<PrjfsDispatcher> dispatcher,
    const folly::Logger* straceLogger,
    ProcessAccessLog& processAccessLog,
    folly::Promise<folly::Unit> deletedPromise,
    std::shared_ptr<Notifier> notifier,
    size_t prjfsTraceBusCapacity)
    : dispatcher_(std::move(dispatcher)),
      straceLogger_(straceLogger),
      notifier_(std::move(notifier)),
      processAccessLog_(processAccessLog),
      deletedPromise_(std::move(deletedPromise)),
      traceDetailedArguments_(std::atomic<size_t>(0)),
      traceBus_(TraceBus<PrjfsTraceEvent>::create(
          "PrjfsTrace",
          prjfsTraceBusCapacity)) {
  traceSubscriptionHandles_.push_back(traceBus_->subscribeFunction(
      "PrjFS request tracking", [this](const PrjfsTraceEvent& event) {
        switch (event.getType()) {
          case PrjfsTraceEvent::START: {
            auto state = telemetryState_.wlock();
            state->requests.emplace(
                event.getData().commandId,
                OutstandingRequest{event.getCallType(), event.getData()});
            break;
          }
          case PrjfsTraceEvent::FINISH: {
            auto state = telemetryState_.wlock();
            auto erased = state->requests.erase(event.getData().commandId);
            XCHECK(erased) << "duplicate prjfs finish event";
            break;
          }
        }
      }));
}

PrjfsChannelInner::~PrjfsChannelInner() {
  if (mountChannel_) {
    PrjStopVirtualizing(mountChannel_);
    deletedPromise_.setValue(folly::unit);
  }
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::waitForPendingNotifications() {
  return dispatcher_->waitForPendingNotifications();
}

HRESULT PrjfsChannelInner::startEnumeration(
    std::shared_ptr<PrjfsRequestContext> context,
    const PRJ_CALLBACK_DATA* callbackData,
    std::unique_ptr<detail::PrjfsLiveRequest> liveRequest,
    const GUID* enumerationId) {
  auto guid = Guid(*enumerationId);
  auto path = RelativePath(callbackData->FilePathName);
  auto fut = makeImmediateFutureWith([this,
                                      context,
                                      guid = std::move(guid),
                                      path = std::move(path)]() mutable {
    auto requestWatch =
        std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(nullptr);
    auto stat = &PrjfsStats::openDir;
    context->startRequest(getStats().copy(), stat, requestWatch);

    FB_LOGF(
        getStraceLogger(), DBG7, "opendir({}, guid={})", path, guid.toString());
    return dispatcher_
        ->opendir(std::move(path), context->getObjectFetchContext())
        .thenValue([this, context = std::move(context), guid = std::move(guid)](
                       auto&& dirents) {
          addDirectoryEnumeration(std::move(guid), std::move(dirents));
          context->sendSuccess();
        });
  });

  detachAndCompleteCallback(
      std::move(fut), std::move(context), std::move(liveRequest));

  return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
}

HRESULT PrjfsChannelInner::endEnumeration(
    std::shared_ptr<PrjfsRequestContext> /* context */,
    const PRJ_CALLBACK_DATA* /* callbackData */,
    std::unique_ptr<detail::PrjfsLiveRequest> /* liveRequest */,
    const GUID* enumerationId) {
  auto guid = Guid(*enumerationId);
  FB_LOGF(getStraceLogger(), DBG7, "closedir({})", guid.toString());

  removeDirectoryEnumeration(guid);

  return S_OK;
}

namespace {
LARGE_INTEGER timespecToPrjTime(EdenTimestamp time) {
  auto filetime = time.toFileTime();
  LARGE_INTEGER res;
  res.QuadPart = (filetime.tv_sec * 1000000000ull + filetime.tv_nsec) / 100ull;
  return res;
}
} // namespace

HRESULT PrjfsChannelInner::getEnumerationData(
    std::shared_ptr<PrjfsRequestContext> context,
    const PRJ_CALLBACK_DATA* callbackData,
    std::unique_ptr<detail::PrjfsLiveRequest> liveRequest,
    const GUID* enumerationId,
    PCWSTR searchExpression,
    PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) {
  auto guid = Guid(*enumerationId);

  FB_LOGF(
      getStraceLogger(),
      DBG7,
      "readdir({}, searchExpression={})",
      guid.toString(),
      searchExpression == nullptr
          ? "<nullptr>"
          : wideToMultibyteString<std::string>(searchExpression));

  auto optEnumerator = findDirectoryEnumeration(guid);
  if (!optEnumerator.has_value()) {
    XLOG(DBG5) << "Directory enumeration not found: " << guid;
    return HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER);
  }
  auto enumerator = std::move(optEnumerator).value();

  auto shouldRestart =
      bool(callbackData->Flags & PRJ_CB_DATA_FLAG_ENUM_RESTART_SCAN);
  if (enumerator->isSearchExpressionEmpty() || shouldRestart) {
    if (searchExpression != nullptr) {
      enumerator->saveExpression(searchExpression);
    } else {
      enumerator->saveExpression(L"*");
    }
  }

  if (shouldRestart) {
    enumerator->restartEnumeration();
  }

  auto fut = makeImmediateFutureWith([this,
                                      context,
                                      enumerator = std::move(enumerator),
                                      buffer = dirEntryBufferHandle]() mutable {
    auto requestWatch =
        std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(nullptr);
    auto stat = &PrjfsStats::readDir;
    context->startRequest(getStats().copy(), stat, requestWatch);

    return enumerator->prepareEnumeration().thenValue(
        [this,
         buffer,
         context = std::move(context),
         enumerator =
             std::move(enumerator)](std::shared_ptr<Enumeration> enumeration) {
          bool added = false;
          auto timestamp = dispatcher_->getLastCheckoutTime();
          auto prjTime = timespecToPrjTime(timestamp);

          for (auto optEntry = enumeration->getCurrent(); optEntry.has_value();
               optEntry = enumeration->getNext()) {
            auto& entry = optEntry.value();

            auto fileInfo = PRJ_FILE_BASIC_INFO();
            fileInfo.IsDirectory = entry.isDir;
            fileInfo.FileSize = entry.size;
            fileInfo.CreationTime = prjTime;
            fileInfo.LastWriteTime = prjTime;
            fileInfo.ChangeTime = prjTime;

            XLOGF(
                DBG6,
                "Directory entry: {}, {}, size={}",
                fileInfo.IsDirectory ? "Dir" : "File",
                PathComponent(entry.name),
                fileInfo.FileSize);

            HRESULT result = 0;
            if (entry.symlinkTarget.has_value()) {
              auto content = entry.symlinkTarget.value();
              fileInfo.FileSize = 0;
              _PRJ_EXTENDED_INFO extInfo;
              extInfo.Symlink.TargetName =
                  std::wstring(content.begin(), content.end()).c_str();
              extInfo.InfoType = _PRJ_EXT_INFO_TYPE::PRJ_EXT_INFO_TYPE_SYMLINK;
              extInfo.NextInfoOffset = 0;
              result = PrjFillDirEntryBuffer2(
                  buffer, entry.name.c_str(), &fileInfo, &extInfo);
            } else {
              result =
                  PrjFillDirEntryBuffer(entry.name.c_str(), &fileInfo, buffer);
            }
            if (FAILED(result)) {
              if (result == HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER) &&
                  added) {
                // We are out of buffer space. This entry didn't make it. Return
                // without increment.
                break;
              } else {
                return folly::Try<folly::Unit>{makeHResultErrorExplicit(
                    result,
                    fmt::format(
                        FMT_STRING("Adding directory entry {}"),
                        PathComponent(entry.name)))};
              }
            }
            added = true;
          }

          context->sendEnumerationSuccess(buffer);
          return folly::Try{folly::unit};
        });
  });

  detachAndCompleteCallback(
      std::move(fut), std::move(context), std::move(liveRequest));

  return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
}

HRESULT PrjfsChannelInner::getPlaceholderInfo(
    std::shared_ptr<PrjfsRequestContext> context,
    const PRJ_CALLBACK_DATA* callbackData,
    std::unique_ptr<detail::PrjfsLiveRequest> liveRequest) {
  auto path = RelativePath(callbackData->FilePathName);
  auto virtualizationContext = callbackData->NamespaceVirtualizationContext;

  auto fut = makeImmediateFutureWith([this,
                                      context,
                                      path = std::move(path),
                                      virtualizationContext]() mutable {
    auto requestWatch =
        std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(nullptr);
    auto stat = &PrjfsStats::lookup;
    context->startRequest(getStats().copy(), stat, requestWatch);

    FB_LOGF(getStraceLogger(), DBG7, "lookup({})", path);
    return dispatcher_
        ->lookup(std::move(path), context->getObjectFetchContext())
        .thenValue(
            [this, context, virtualizationContext = virtualizationContext](
                std::optional<LookupResult>&& optLookupResult)
                -> ImmediateFuture<folly::Unit> {
              if (!optLookupResult) {
                context->sendError(HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND));
                return folly::unit;
              }
              const auto& lookupResult = optLookupResult.value();

              auto timestamp = dispatcher_->getLastCheckoutTime();
              auto prjTime = timespecToPrjTime(timestamp);

              PRJ_PLACEHOLDER_INFO placeholderInfo{};
              placeholderInfo.FileBasicInfo.IsDirectory = lookupResult.isDir;
              placeholderInfo.FileBasicInfo.FileSize = lookupResult.size;
              placeholderInfo.FileBasicInfo.CreationTime = prjTime;
              placeholderInfo.FileBasicInfo.LastWriteTime = prjTime;
              placeholderInfo.FileBasicInfo.ChangeTime = prjTime;
              auto inodeName = lookupResult.path.wide();

              HRESULT result;
              if (symlinksSupported() &&
                  lookupResult.symlinkDestination.has_value()) {
                std::string content = lookupResult.symlinkDestination.value();
                std::wstring wcontent =
                    std::wstring(content.begin(), content.end());
                auto targetName = std::wstring(content.begin(), content.end());
                _PRJ_EXTENDED_INFO extInfo;
                extInfo.Symlink.TargetName = targetName.c_str();
                extInfo.InfoType =
                    _PRJ_EXT_INFO_TYPE::PRJ_EXT_INFO_TYPE_SYMLINK;
                extInfo.NextInfoOffset = 0;
                result = PrjWritePlaceholderInfo2(
                    virtualizationContext,
                    inodeName.c_str(),
                    &placeholderInfo,
                    sizeof(placeholderInfo),
                    &extInfo);
              } else {
                result = PrjWritePlaceholderInfo(
                    virtualizationContext,
                    inodeName.c_str(),
                    &placeholderInfo,
                    sizeof(placeholderInfo));
              }

              if (FAILED(result)) {
                return makeImmediateFuture<folly::Unit>(
                    makeHResultErrorExplicit(
                        result,
                        fmt::format(
                            FMT_STRING("Writing placeholder for {}"),
                            lookupResult.path)));
              }

              context->sendSuccess();

              return folly::unit;
            });
  });

  detachAndCompleteCallback(
      std::move(fut), std::move(context), std::move(liveRequest));

  return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
}

HRESULT PrjfsChannelInner::queryFileName(
    std::shared_ptr<PrjfsRequestContext> context,
    const PRJ_CALLBACK_DATA* callbackData,
    std::unique_ptr<detail::PrjfsLiveRequest> liveRequest) {
  auto path = RelativePath(callbackData->FilePathName);

  auto fut = makeImmediateFutureWith([this,
                                      context,
                                      path = std::move(path)]() mutable {
    auto requestWatch =
        std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(nullptr);
    auto stat = &PrjfsStats::access;
    context->startRequest(getStats().copy(), stat, requestWatch);
    FB_LOGF(getStraceLogger(), DBG7, "access({})", path);
    return dispatcher_
        ->access(std::move(path), context->getObjectFetchContext())
        .thenValue([context = std::move(context)](bool present) {
          if (present) {
            context->sendSuccess();
          } else {
            context->sendError(HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND));
          }
        });
  });

  detachAndCompleteCallback(
      std::move(fut), std::move(context), std::move(liveRequest));

  return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
}

namespace {

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

} // namespace

HRESULT PrjfsChannelInner::getFileData(
    std::shared_ptr<PrjfsRequestContext> context,
    const PRJ_CALLBACK_DATA* callbackData,
    std::unique_ptr<detail::PrjfsLiveRequest> liveRequest,
    UINT64 byteOffset,
    UINT32 length) {
  auto fut = makeImmediateFutureWith(
      [this,
       context,
       path = RelativePath(callbackData->FilePathName),
       virtualizationContext = callbackData->NamespaceVirtualizationContext,
       dataStreamId = Guid(callbackData->DataStreamId),
       byteOffset,
       length]() mutable {
        auto requestWatch =
            std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(
                nullptr);
        auto stat = &PrjfsStats::read;
        context->startRequest(getStats().copy(), stat, requestWatch);

        FB_LOGF(
            getStraceLogger(),
            DBG7,
            "read({}, off={}, len={})",
            path,
            byteOffset,
            length);
        return dispatcher_
            ->read(std::move(path), context->getObjectFetchContext())
            .thenValue([context = std::move(context),
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
                      startOffset + kMaxChunkSize, instanceInfo.WriteAlignment);
                  XDCHECK_GT(endOffset, 0ul);
                  XDCHECK_GT(endOffset, startOffset);

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
      });

  detachAndCompleteCallback(
      std::move(fut), std::move(context), std::move(liveRequest));

  return HRESULT_FROM_WIN32(ERROR_IO_PENDING);
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::matchEdenViewOfFileToFS(
    RelativePath relPath,
    const ObjectFetchContextPtr& context) {
  return ImmediateFuture{
      dispatcher_->matchEdenViewOfFileToFS(std::move(relPath), context)
          .semi()
          .via(dispatcher_->getNotificationExecutor())
          .semi()};
}

std::vector<PrjfsChannelInner::OutstandingRequest>
PrjfsChannelInner::getOutstandingRequests() {
  std::vector<PrjfsChannelInner::OutstandingRequest> outstandingCalls;

  auto telemetryStateLockedPtr = telemetryState_.rlock();
  for (const auto& entry : telemetryStateLockedPtr->requests) {
    outstandingCalls.push_back(entry.second);
  }
  return outstandingCalls;
}

TraceDetailedArgumentsHandle PrjfsChannelInner::traceDetailedArguments() {
  // We could implement something fancier here that just copies the shared_ptr
  // into a handle struct that increments upon taking ownership and decrements
  // on destruction, but this code path is quite rare, so do the expedient
  // thing.
  auto handle =
      std::shared_ptr<void>(nullptr, [&copy = traceDetailedArguments_](void*) {
        copy.fetch_sub(1, std::memory_order_acq_rel);
      });
  traceDetailedArguments_.fetch_add(1, std::memory_order_acq_rel);
  return handle;
};

namespace {
typedef ImmediateFuture<folly::Unit> (PrjfsChannelInner::*NotificationHandler)(
    RelativePath oldPath,
    RelativePath destPath,
    bool isDirectory,
    const ObjectFetchContextPtr& context);

typedef std::string (*NotificationArgRenderer)(
    RelativePathPiece relPath,
    RelativePathPiece destPath,
    bool isDirectory);

struct NotificationHandlerEntry {
  constexpr NotificationHandlerEntry(
      NotificationHandler h,
      NotificationArgRenderer r,
      PrjfsStats::DurationPtr s)
      : handler{h}, renderer{r}, stat{s} {}

  NotificationHandler handler;
  NotificationArgRenderer renderer;
  PrjfsStats::DurationPtr stat;
};

std::string newFileCreatedRenderer(
    RelativePathPiece relPath,
    RelativePathPiece /*destPath*/,
    bool isDirectory) {
  return fmt::format(
      FMT_STRING("{}Created({})"), isDirectory ? "dir" : "file", relPath);
}

std::string fileOverwrittenRenderer(
    RelativePathPiece relPath,
    RelativePathPiece /*destPath*/,
    bool /*isDirectory*/) {
  return fmt::format(FMT_STRING("fileOverwritten({})"), relPath);
}

std::string fileHandleClosedFileModifiedRenderer(
    RelativePathPiece relPath,
    RelativePathPiece /*destPath*/,
    bool /*isDirectory*/) {
  return fmt::format(FMT_STRING("fileModified({})"), relPath);
}

std::string fileRenamedRenderer(
    RelativePathPiece oldPath,
    RelativePathPiece newPath,
    bool /*isDirectory*/) {
  return fmt::format(FMT_STRING("fileRenamed({} -> {})"), oldPath, newPath);
}

std::string preRenameRenderer(
    RelativePathPiece oldPath,
    RelativePathPiece newPath,
    bool /*isDirectory*/) {
  return fmt::format(FMT_STRING("preRename({} -> {})"), oldPath, newPath);
}

std::string fileHandleClosedFileDeletedRenderer(
    RelativePathPiece relPath,
    RelativePathPiece /*destPath*/,
    bool isDirectory) {
  return fmt::format(
      FMT_STRING("{}Deleted({})"), isDirectory ? "dir" : "file", relPath);
}

std::string preDeleteRenderer(
    RelativePathPiece relPath,
    RelativePathPiece /*destPath*/,
    bool isDirectory) {
  return fmt::format(
      FMT_STRING("pre{}Deleted({})"), isDirectory ? "Dir" : "File", relPath);
}

std::string preSetHardlinkRenderer(
    RelativePathPiece oldPath,
    RelativePathPiece newPath,
    bool /*isDirectory*/) {
  return fmt::format(FMT_STRING("link({} -> {})"), oldPath, newPath);
}

std::string preConvertToFullRenderer(
    RelativePathPiece relPath,
    RelativePathPiece /*destPath*/,
    bool isDirectory) {
  return fmt::format(
      FMT_STRING("preConvertToFull({}, isDirectory={})"), relPath, isDirectory);
}

const std::unordered_map<PRJ_NOTIFICATION, NotificationHandlerEntry>
    notificationHandlerMap = {
        {
            PRJ_NOTIFICATION_NEW_FILE_CREATED,
            {&PrjfsChannelInner::newFileCreated,
             newFileCreatedRenderer,
             &PrjfsStats::newFileCreated},
        },
        {
            PRJ_NOTIFICATION_PRE_DELETE,
            {&PrjfsChannelInner::preDelete,
             preDeleteRenderer,
             &PrjfsStats::preDelete},
        },
        {
            PRJ_NOTIFICATION_FILE_OVERWRITTEN,
            {&PrjfsChannelInner::fileOverwritten,
             fileOverwrittenRenderer,
             &PrjfsStats::fileOverwritten},
        },
        {
            PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_MODIFIED,
            {&PrjfsChannelInner::fileHandleClosedFileModified,
             fileHandleClosedFileModifiedRenderer,
             &PrjfsStats::fileHandleClosedFileModified},
        },
        {
            PRJ_NOTIFICATION_FILE_RENAMED,
            {&PrjfsChannelInner::fileRenamed,
             fileRenamedRenderer,
             &PrjfsStats::fileRenamed},
        },
        {
            PRJ_NOTIFICATION_PRE_RENAME,
            {&PrjfsChannelInner::preRename,
             preRenameRenderer,
             &PrjfsStats::preRenamed},
        },
        {
            PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_DELETED,
            {&PrjfsChannelInner::fileHandleClosedFileDeleted,
             fileHandleClosedFileDeletedRenderer,
             &PrjfsStats::fileHandleClosedFileDeleted},
        },
        {
            PRJ_NOTIFICATION_PRE_SET_HARDLINK,
            {&PrjfsChannelInner::preSetHardlink,
             preSetHardlinkRenderer,
             &PrjfsStats::preSetHardlink},
        },
        {
            PRJ_NOTIFICATION_FILE_PRE_CONVERT_TO_FULL,
            {&PrjfsChannelInner::preConvertToFull,
             preConvertToFullRenderer,
             &PrjfsStats::preConvertToFull},
        },
};
} // namespace

ImmediateFuture<folly::Unit> PrjfsChannelInner::newFileCreated(
    RelativePath relPath,
    RelativePath /*destPath*/,
    bool isDirectory,
    const ObjectFetchContextPtr& context) {
  if (isDirectory) {
    return dispatcher_->dirCreated(std::move(relPath), context);
  } else {
    return dispatcher_->fileCreated(std::move(relPath), context);
  }
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::fileOverwritten(
    RelativePath relPath,
    RelativePath /*destPath*/,
    bool /*isDirectory*/,
    const ObjectFetchContextPtr& context) {
  return dispatcher_->fileModified(std::move(relPath), context);
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::fileHandleClosedFileModified(
    RelativePath relPath,
    RelativePath /*destPath*/,
    bool /*isDirectory*/,
    const ObjectFetchContextPtr& context) {
  return dispatcher_->fileModified(std::move(relPath), context);
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::fileRenamed(
    RelativePath oldPath,
    RelativePath newPath,
    bool isDirectory,
    const ObjectFetchContextPtr& context) {
  // When files are moved in and out of the repo, the rename paths are
  // empty, handle these like creation/removal of files.
  if (oldPath.empty()) {
    return newFileCreated(
        std::move(newPath), RelativePath{}, isDirectory, context);
  } else if (newPath.empty()) {
    return fileHandleClosedFileDeleted(
        std::move(oldPath), RelativePath{}, isDirectory, context);
  } else {
    return dispatcher_->fileRenamed(
        std::move(oldPath), std::move(newPath), context);
  }
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::preRename(
    RelativePath oldPath,
    RelativePath newPath,
    bool isDirectory,
    const ObjectFetchContextPtr& context) {
  if (isDirectory) {
    return dispatcher_->preDirRename(
        std::move(oldPath), std::move(newPath), context);
  } else {
    return dispatcher_->preFileRename(
        std::move(oldPath), std::move(newPath), context);
  }
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::preDelete(
    RelativePath relPath,
    RelativePath /*destPath*/,
    bool isDirectory,
    const ObjectFetchContextPtr& context) {
  if (isDirectory) {
    return dispatcher_->preDirDelete(std::move(relPath), context);
  } else {
    return dispatcher_->preFileDelete(std::move(relPath), context);
  }
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::fileHandleClosedFileDeleted(
    RelativePath oldPath,
    RelativePath /*destPath*/,
    bool isDirectory,
    const ObjectFetchContextPtr& context) {
  if (isDirectory) {
    return dispatcher_->dirDeleted(std::move(oldPath), context);
  } else {
    return dispatcher_->fileDeleted(std::move(oldPath), context);
  }
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::preSetHardlink(
    RelativePath relPath,
    RelativePath /*newPath*/,
    bool /*isDirectory*/,
    const ObjectFetchContextPtr& /*context*/) {
  return folly::Try<folly::Unit>(makeHResultErrorExplicit(
      HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED),
      fmt::format(FMT_STRING("Hardlinks are not supported: {}"), relPath)));
}

ImmediateFuture<folly::Unit> PrjfsChannelInner::preConvertToFull(
    RelativePath relpath,
    RelativePath /*destPath*/,
    bool /*isDirectory*/,
    const ObjectFetchContextPtr& context) {
  return dispatcher_->preFileConvertedToFull(std::move(relpath), context);
}

HRESULT PrjfsChannelInner::notification(
    std::shared_ptr<PrjfsRequestContext> context,
    const PRJ_CALLBACK_DATA* callbackData,
    BOOLEAN isDirectory,
    PRJ_NOTIFICATION notificationType,
    PCWSTR destinationFileName,
    PRJ_NOTIFICATION_PARAMETERS* /*notificationParameters*/) {
  auto it = notificationHandlerMap.find(notificationType);
  if (it == notificationHandlerMap.end()) {
    XLOG(WARN) << "Unrecognized notification: " << notificationType;
    return HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER);
  } else {
    auto stat = it->second.stat;
    auto handler = it->second.handler;
    auto renderer = it->second.renderer;

    auto relPath = RelativePath(callbackData->FilePathName);
    auto destPath = RelativePath(destinationFileName);

    // The underlying handlers may call into the inode code and since this
    // notification may have been triggered by the inode code itself, we may
    // end up in a deadlock. To prevent this, let's simply bail here when this
    // happens.
    if (callbackData->TriggeringProcessId == GetCurrentProcessId()) {
      XLOG(ERR) << "Recursive EdenFS call are disallowed for: " << relPath;
      return HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);
    }

    auto requestWatch =
        std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>(nullptr);
    context->startRequest(getStats().copy(), stat, requestWatch);

    FB_LOG(getStraceLogger(), DBG7, renderer(relPath, destPath, isDirectory));
    auto fut = (this->*handler)(
                   std::move(relPath),
                   std::move(destPath),
                   isDirectory,
                   context->getObjectFetchContext())
                   .semi();
    if (fut.isReady()) {
      // The notification is ready, this is usually coming from pre*
      // notifications to deny the operation, in that case EdenFS should return
      // the error code instead of pushing the operation to the background.
      return tryToHResult(std::move(fut).getTry(0ms));
    } else {
      folly::futures::detachOn(
          dispatcher_->getNotificationExecutor(),
          std::move(fut).deferEnsure([context] {}));
      return S_OK;
    }
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

void PrjfsChannelInner::sendSuccess(
    int32_t commandId,
    PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS* FOLLY_NULLABLE extra) {
  sendReply(mountChannel_, commandId, S_OK, extra);
}

void PrjfsChannelInner::sendError(int32_t commandId, HRESULT result) {
  sendReply(mountChannel_, commandId, result, nullptr);
}

PrjfsChannel::PrjfsChannel(
    AbsolutePathPiece mountPath,
    std::unique_ptr<PrjfsDispatcher> dispatcher,
    std::shared_ptr<ReloadableConfig> config,
    const folly::Logger* straceLogger,
    std::shared_ptr<ProcessInfoCache> processInfoCache,
    Guid guid,
    bool enableWindowsSymlinks,
    std::shared_ptr<Notifier> notifier)
    : mountPath_(mountPath),
      mountId_(std::move(guid)),
      enableSymlinks_(enableWindowsSymlinks),
      processAccessLog_(std::move(processInfoCache)),
      config_{std::move(config)} {
  auto [innerDeletedPromise, innerDeletedFuture] =
      folly::makePromiseContract<folly::Unit>();
  innerDeleted_ = std::move(innerDeletedFuture);
  inner_.store(std::make_shared<PrjfsChannelInner>(
      std::move(dispatcher),
      straceLogger,
      processAccessLog_,
      std::move(innerDeletedPromise),
      std::move(notifier),
      config_->getEdenConfig()->PrjfsTraceBusCapacity.getValue()));
}

PrjfsChannel::~PrjfsChannel() {
  XCHECK(stopPromise_.isFulfilled())
      << "stop() must be called before destroying the channel";
}

void PrjfsChannel::destroy() {
  delete this;
}

folly::Future<FsChannel::StopFuture> PrjfsChannel::initialize() {
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
           PRJ_NOTIFY_PRE_DELETE | PRJ_NOTIFY_PRE_RENAME |
           PRJ_NOTIFY_FILE_RENAMED |
           PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_MODIFIED |
           PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_DELETED |
           PRJ_NOTIFY_PRE_SET_HARDLINK,
       L""},
  };

  auto config = config_->getEdenConfig();
  if (config->prjfsListenToPreConvertToFull.getValue()) {
    notificationMappings[0].NotificationBitMask |=
        PRJ_NOTIFY_FILE_PRE_CONVERT_TO_FULL;
  }

  auto startOpts = PRJ_STARTVIRTUALIZING_OPTIONS();
  startOpts.NotificationMappings = notificationMappings;
  startOpts.NotificationMappingsCount =
      folly::to_narrow(std::size(notificationMappings));

  useNegativePathCaching_ = config->prjfsUseNegativePathCaching.getValue();
  if (useNegativePathCaching_) {
    startOpts.Flags = PRJ_FLAG_USE_NEGATIVE_PATH_CACHE;
  }

  XLOGF(
      INFO,
      "Starting PrjfsChannel for: {} with GUID: {}",
      mountPath_,
      mountId_);

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

  PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT mountChannel;
  result = PrjStartVirtualizing(
      winPath.c_str(), &callbacks, this, &startOpts, &mountChannel);

  if (FAILED(result)) {
    throw makeHResultErrorExplicit(result, "Failed to start the mount point");
  }

  if (enableSymlinks_) {
    getInner()->initializeSymlinkSupport();
  }

  getInner()->setMountChannel(mountChannel);

  // On Windows, negative path cache is kept between channels. Invalidating here
  // gives our user an easy way to get out of a situation where an incorrect
  // negative path result is cached by Windows without rebooting.
  flushNegativePathCache();

  XLOG(INFO) << "Started PrjfsChannel for: " << mountPath_;

  stopPromise_ = folly::Promise<FsStopDataPtr>{};
  return folly::makeFuture<FsChannel::StopFuture>(getStopFuture());
}

ImmediateFuture<folly::Unit> PrjfsChannel::waitForPendingWrites() {
  auto inner = getInner();
  if (!inner) {
    return makeImmediateFuture<folly::Unit>(std::runtime_error(fmt::format(
        FMT_STRING("The mount at {} has been stopped"), mountPath_)));
  }
  return inner->waitForPendingNotifications().ensure(
      [inner = std::move(inner)] {});
}

ImmediateFuture<folly::Unit> PrjfsChannel::matchEdenViewOfFileToFS(
    RelativePath relPath,
    const ObjectFetchContextPtr& context) {
  auto inner = getInner();
  if (!inner) {
    return makeImmediateFuture<folly::Unit>(std::runtime_error(fmt::format(
        FMT_STRING("The mount at {} has been stopped"), mountPath_)));
  }
  return inner->matchEdenViewOfFileToFS(std::move(relPath), context)
      .ensure([inner = std::move(inner)] {});
}

bool PrjfsChannel::StopData::isUnmounted() {
  return true;
}

FsChannelInfo PrjfsChannel::StopData::extractTakeoverInfo() {
  return ProjFsChannelData{};
}

folly::SemiFuture<folly::Unit> PrjfsChannel::unmount() {
  XLOG(INFO) << "Stopping PrjfsChannel for: " << mountPath_;
  XCHECK(!stopPromise_.isFulfilled());

  inner_.store(nullptr, std::memory_order_release);
  return std::move(innerDeleted_)
      .deferValue([stopPromise = std::move(stopPromise_)](auto&&) mutable {
        stopPromise.setValue(std::make_unique<StopData>());
      });
}

FsChannel::StopFuture PrjfsChannel::getStopFuture() {
  return stopPromise_.getSemiFuture();
}

// TODO: We need to add an extra layer to absorb all the exceptions generated in
// Eden from leaking into FS. This would come in soon.

folly::Try<folly::Unit> PrjfsChannel::removeCachedFile(RelativePathPiece path) {
  auto inner = getInner();
  if (!inner) {
    // TODO: The mount is being unmounted but the caller is still manipulating
    // it. This is unexpected.
    return folly::Try<folly::Unit>{std::runtime_error{fmt::format(
        FMT_STRING("Couldn't delete file {}: PrjfsChannel is stopped"), path)}};
  }

  DurationScope statScope{inner->getStats(), &PrjfsStats::removeCachedFile};

  if (path.empty()) {
    return folly::Try<folly::Unit>{folly::unit};
  }

  auto winPath = path.wide();

  XLOG(DBG6) << "Invalidating: " << path;

  PRJ_UPDATE_FAILURE_CAUSES failureReason;
  auto result = PrjDeleteFile(
      inner->getMountChannel(),
      winPath.c_str(),
      PRJ_UPDATE_ALLOW_DIRTY_METADATA | PRJ_UPDATE_ALLOW_DIRTY_DATA |
          PRJ_UPDATE_ALLOW_READ_ONLY | PRJ_UPDATE_ALLOW_TOMBSTONE,
      &failureReason);
  if (FAILED(result)) {
    if (result == HRESULT_FROM_WIN32(ERROR_REPARSE_POINT_ENCOUNTERED)) {
      // We've attempted to call PrjDeleteFile on a directory. That isn't
      // supported, let's just ignore.
    } else if (
        result == HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND) ||
        result == HRESULT_FROM_WIN32(ERROR_PATH_NOT_FOUND)) {
      // The file or a directory in the path is not cached, ignore.
    } else if (result == HRESULT_FROM_WIN32(ERROR_DIR_NOT_EMPTY)) {
      return folly::Try<folly::Unit>{
          std::system_error(ENOTEMPTY, std::generic_category())};
    } else {
      return folly::Try<folly::Unit>{makeHResultErrorExplicit(
          result,
          fmt::format(
              FMT_STRING("Couldn't delete file {}: {:#x}"),
              path,
              static_cast<uint32_t>(result)))};
    }
  }

  return folly::Try<folly::Unit>{folly::unit};
}

folly::Try<folly::Unit> PrjfsChannel::addDirectoryPlaceholder(
    RelativePathPiece path) {
  auto inner = getInner();
  if (!inner) {
    return folly::Try<folly::Unit>{std::runtime_error{fmt::format(
        FMT_STRING(
            "Couldn't add a placeholder for {}: PrjfsChannel is stopped"),
        path)}};
  }

  DurationScope statScope{
      inner->getStats(), &PrjfsStats::addDirectoryPlaceholder};

  if (path.empty()) {
    return folly::Try<folly::Unit>{folly::unit};
  }

  auto winMountPath = mountPath_.wide();
  auto fullPath = mountPath_ + path;
  auto winPath = fullPath.wide();

  XLOGF(DBG6, "Adding a placeholder for: {}", path);
  auto result = PrjMarkDirectoryAsPlaceholder(
      winMountPath.c_str(), winPath.c_str(), nullptr, mountId_);
  if (FAILED(result)) {
    if (result == HRESULT_FROM_WIN32(ERROR_REPARSE_POINT_ENCOUNTERED)) {
      // This is already a placeholder, not an error.
    } else if (
        result == HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND) ||
        result == HRESULT_FROM_WIN32(ERROR_PATH_NOT_FOUND)) {
      // If EdenFS happens to be invalidating a directory that is no longer
      // present in the destination commit, PrjMarkDirectoryAsPlaceholder would
      // trigger a recursive lookup call and fail, raising this error. This is
      // harmless and thus we can just ignore.
    } else {
      return folly::Try<folly::Unit>{makeHResultErrorExplicit(
          result,
          fmt::format(
              FMT_STRING("Couldn't add a placeholder for {}: {:#x}"),
              path,
              static_cast<uint32_t>(result)))};
    }
  }

  return folly::Try<folly::Unit>{folly::unit};
}

ImmediateFuture<folly::Unit> PrjfsChannel::completeInvalidations() {
  // completeInvalidations() is called before filesystem-modifying Thrift calls
  // return. If new files have been added, we need to clear the negative path
  // cache.
  flushNegativePathCache();
  return folly::unit;
}

void PrjfsChannel::flushNegativePathCache() {
  auto inner = getInner();
  if (!inner) {
    return;
  }

  if (useNegativePathCaching_) {
    XLOG(DBG6) << "Flushing negative path cache";

    uint32_t numFlushed = 0;
    auto result =
        PrjClearNegativePathCache(inner->getMountChannel(), &numFlushed);
    if (FAILED(result)) {
      throwHResultErrorExplicit(
          result, "Couldn't flush the negative path cache");
    }

    XLOGF(DBG6, "Flushed {} entries", numFlushed);
  }
}

void PrjfsChannelInner::initializeSymlinkSupport() {
  if (placeholderExtendedInfo2_ == nullptr) {
    placeholderExtendedInfo2_ = (PPWPI2)GetProcAddress(
        GetModuleHandle(TEXT("ProjectedFSLib.dll")),
        "PrjWritePlaceholderInfo2");
  }
  if (prjFillDirEntryBuffer2_ == nullptr) {
    prjFillDirEntryBuffer2_ = (PPFDEB2)GetProcAddress(
        GetModuleHandle(TEXT("ProjectedFSLib.dll")), "PrjFillDirEntryBuffer2");
  }
  if (placeholderExtendedInfo2_ == nullptr ||
      prjFillDirEntryBuffer2_ == nullptr) {
    throw makeHResultErrorExplicit(
        255,
        "Failed to start the mount point: support for symlink requested but PrjFS does not support symlinks in the current system");
  }
  symlinksSupported_ = true;
}

} // namespace facebook::eden

#endif
