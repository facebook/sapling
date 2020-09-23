/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h> // @manual
#include "eden/fs/inodes/RequestContext.h"
#include "eden/fs/prjfs/PrjfsChannel.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class PrjfsRequestContext : public RequestContext {
 public:
  PrjfsRequestContext(const PrjfsRequestContext&) = delete;
  PrjfsRequestContext& operator=(const PrjfsRequestContext&) = delete;
  PrjfsRequestContext(PrjfsRequestContext&&) = delete;
  PrjfsRequestContext& operator=(PrjfsRequestContext&&) = delete;

  explicit PrjfsRequestContext(
      PrjfsChannel* channel,
      const PRJ_CALLBACK_DATA& prjfsData)
      : RequestContext(channel->getProcessAccessLog()),
        channel_(channel),
        commandId_(prjfsData.CommandId) {}

  folly::Future<folly::Unit> catchErrors(folly::Future<folly::Unit>&& fut) {
    return std::move(fut).thenTryInline([this](folly::Try<folly::Unit>&& try_) {
      SCOPE_EXIT {
        finishRequest();
      };

      if (try_.hasException()) {
        auto* err = try_.tryGetExceptionObject<std::exception>();
        DCHECK(err);
        sendError(exceptionToHResult(*err));
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

  void sendError(HRESULT result) const {
    return channel_->sendError(commandId_, result);
  }

 private:
  PrjfsChannel* channel_;
  int32_t commandId_;
};

} // namespace facebook::eden
