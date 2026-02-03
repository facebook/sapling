/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/prjfs/PrjfsRequestContext.h"

#include <fmt/core.h>
#include <thrift/lib/cpp/util/EnumUtils.h>

namespace facebook::eden {

PrjfsRequestContext::PrjfsRequestContext(
    folly::ReadMostlySharedPtr<PrjfsChannelInner> channel,
    const PRJ_CALLBACK_DATA& prjfsData,
    PrjfsTraceCallType callType,
    LPCWSTR destinationFileName)
    : RequestContext(
          channel->getProcessAccessLog(),
          channel->getStructuredLogger(),
          channel->getLongRunningFSRequestThreshold(),
          makeRefPtr<PrjfsObjectFetchContext>(
              ProcessId{prjfsData.TriggeringProcessId})),
      channel_(std::move(channel)),
      commandId_(prjfsData.CommandId),
      callType_{callType},
      data_{prjfsData} {
  if (channel_->getTraceDetailedArguments().load(std::memory_order_acquire)) {
    channel_->getTraceBusPtr()->publish(
        PrjfsTraceEvent::start(
            callType_,
            data_,
            formatTraceEventString(
                callType_, data_, prjfsData, destinationFileName)));
  } else {
    channel_->getTraceBusPtr()->publish(
        PrjfsTraceEvent::start(callType_, data_));
  }
}

PrjfsRequestContext::~PrjfsRequestContext() {
  if (channel_) {
    channel_->getTraceBusPtr()->publish(
        PrjfsTraceEvent::finish(callType_, data_));
  }
}

folly::ReadMostlyWeakPtr<PrjfsChannelInner>
PrjfsRequestContext::getChannelForAsyncUse() {
  return folly::ReadMostlyWeakPtr<PrjfsChannelInner>{channel_};
}

ImmediateFuture<folly::Unit> PrjfsRequestContext::catchErrors(
    ImmediateFuture<folly::Unit>&& fut,
    EdenStatsPtr stats,
    StatsGroupBase::Counter PrjfsStats::* countSuccessful,
    StatsGroupBase::Counter PrjfsStats::* countFailure) {
  return std::move(fut).thenTry(
      [this, stats = std::move(stats), countSuccessful, countFailure](
          folly::Try<folly::Unit>&& try_) {
        auto result = tryToHResult(try_);
        if (result != S_OK) {
          if (stats && countFailure) {
            stats->increment(countFailure);
          }
          sendError(result);
        } else {
          if (stats && countSuccessful) {
            stats->increment(countSuccessful);
          }
        }
      });
}

void PrjfsRequestContext::sendSuccess() const {
  result_ = S_OK;
  return channel_->sendSuccess(commandId_, nullptr);
}

void PrjfsRequestContext::sendNotificationSuccess() const {
  result_ = S_OK;
  PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS extra{};
  extra.CommandType = PRJ_COMPLETE_COMMAND_TYPE_NOTIFICATION;
  return channel_->sendSuccess(commandId_, &extra);
}

void PrjfsRequestContext::sendEnumerationSuccess(
    PRJ_DIR_ENTRY_BUFFER_HANDLE buffer) const {
  result_ = S_OK;
  PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS extra{};
  extra.CommandType = PRJ_COMPLETE_COMMAND_TYPE_ENUMERATION;
  extra.Enumeration.DirEntryBufferHandle = buffer;
  return channel_->sendSuccess(commandId_, &extra);
}

void PrjfsRequestContext::sendError(HRESULT result) const {
  result_ = result;
  return channel_->sendError(commandId_, result);
}

std::string PrjfsRequestContext::formatTraceEventString(
    PrjfsTraceCallType callType,
    const PrjfsTraceEvent::PrjfsOperationData& data,
    const PRJ_CALLBACK_DATA& prjfsData,
    LPCWSTR destinationFileName) {
  // Most events only have data.FilePathName set to a repo-relative path,
  // describing the file that is related to the event.
  //
  // This path can be the empty string L"" if the operation is in the repo
  // root directory, such as `dir %REPO_ROOT%`. In these cases,
  // destinationFileName is nullptr, either pass explicitly in this codebase,
  // or given to the ::notification() function (which is implementation of
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
  LPCWSTR relativeFileName = prjfsData.FilePathName;
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
      data.commandId,
      processPathToName(prjfsData.TriggeringProcessImageFileName),
      data.pid,
      apache::thrift::util::enumName(callType, "(unknown)"),
      relativeFileName == nullptr ? RelativePath{}
                                  : RelativePath(relativeFileName),
      (destinationFileName && relativeFileName) ? "=>" : "",
      destinationFileName == nullptr ? RelativePath{}
                                     : RelativePath(destinationFileName));
}

std::string PrjfsRequestContext::processPathToName(PCWSTR fullAppName) {
  if (fullAppName == nullptr) {
    return "None";
  } else {
    auto appName = basenameFromAppName(fullAppName);
    return wideToMultibyteString<std::string>(appName);
  }
}

std::wstring_view PrjfsRequestContext::basenameFromAppName(PCWSTR fullAppName) {
  auto fullAppNameView = std::wstring_view(fullAppName);
  auto lastBackslash = fullAppNameView.find_last_of(L'\\');
  if (lastBackslash == std::wstring_view::npos) {
    return fullAppNameView;
  }
  return fullAppNameView.substr(lastBackslash + 1);
}

} // namespace facebook::eden
