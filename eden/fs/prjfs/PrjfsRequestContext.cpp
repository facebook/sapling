/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/prjfs/PrjfsRequestContext.h"

namespace facebook::eden {

PrjfsRequestContext::PrjfsRequestContext(
    folly::ReadMostlySharedPtr<PrjfsChannelInner> channel,
    const PRJ_CALLBACK_DATA& prjfsData)
    : RequestContext(
          channel->getProcessAccessLog(),
          channel->getStructuredLogger(),
          channel->getLongRunningFSRequestThreshold(),
          makeRefPtr<PrjfsObjectFetchContext>(
              ProcessId{prjfsData.TriggeringProcessId})),
      channel_(std::move(channel)),
      commandId_(prjfsData.CommandId) {}

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
  return channel_->sendSuccess(commandId_, nullptr);
}

void PrjfsRequestContext::sendNotificationSuccess() const {
  PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS extra{};
  extra.CommandType = PRJ_COMPLETE_COMMAND_TYPE_NOTIFICATION;
  return channel_->sendSuccess(commandId_, &extra);
}

void PrjfsRequestContext::sendEnumerationSuccess(
    PRJ_DIR_ENTRY_BUFFER_HANDLE buffer) const {
  PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS extra{};
  extra.CommandType = PRJ_COMPLETE_COMMAND_TYPE_ENUMERATION;
  extra.Enumeration.DirEntryBufferHandle = buffer;
  return channel_->sendSuccess(commandId_, &extra);
}

void PrjfsRequestContext::sendError(HRESULT result) const {
  return channel_->sendError(commandId_, result);
}

} // namespace facebook::eden
