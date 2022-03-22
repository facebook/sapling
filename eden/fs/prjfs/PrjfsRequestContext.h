/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h> // @manual
#include "eden/fs/inodes/RequestContext.h"
#include "eden/fs/prjfs/PrjfsChannel.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class PrjfsRequestContext : public RequestContext {
 public:
  PrjfsRequestContext(const PrjfsRequestContext&) = delete;
  PrjfsRequestContext& operator=(const PrjfsRequestContext&) = delete;
  PrjfsRequestContext(PrjfsRequestContext&&) = delete;
  PrjfsRequestContext& operator=(PrjfsRequestContext&&) = delete;

  explicit PrjfsRequestContext(
      folly::ReadMostlySharedPtr<PrjfsChannelInner> channel,
      const PRJ_CALLBACK_DATA& prjfsData)
      : RequestContext(channel->getProcessAccessLog()),
        channel_(std::move(channel)),
        commandId_(prjfsData.CommandId),
        clientPid_(prjfsData.TriggeringProcessId) {}

  std::optional<pid_t> getClientPid() const override {
    return clientPid_;
  }

  ImmediateFuture<folly::Unit> catchErrors(ImmediateFuture<folly::Unit>&& fut) {
    return std::move(fut).thenTry([this](folly::Try<folly::Unit>&& try_) {
      auto result = tryToHResult(try_);
      if (result != S_OK) {
        sendError(result);
      }
    });
  }

  void sendSuccess() const {
    return channel_->sendSuccess(commandId_, nullptr);
  }

  void sendNotificationSuccess() const {
    PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS extra{};
    extra.CommandType = PRJ_COMPLETE_COMMAND_TYPE_NOTIFICATION;
    return channel_->sendSuccess(commandId_, &extra);
  }

  void sendEnumerationSuccess(PRJ_DIR_ENTRY_BUFFER_HANDLE buffer) const {
    PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS extra{};
    extra.CommandType = PRJ_COMPLETE_COMMAND_TYPE_ENUMERATION;
    extra.Enumeration.DirEntryBufferHandle = buffer;
    return channel_->sendSuccess(commandId_, &extra);
  }

  void sendError(HRESULT result) const {
    return channel_->sendError(commandId_, result);
  }

 private:
  folly::ReadMostlySharedPtr<PrjfsChannelInner> channel_;
  int32_t commandId_;
  pid_t clientPid_;
};

} // namespace facebook::eden
