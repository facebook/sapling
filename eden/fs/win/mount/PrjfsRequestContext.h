/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h>
#include "eden/fs/inodes/RequestContext.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/mount/PrjfsChannel.h"

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

  int32_t getCommandId() const {
    return commandId_;
  }

 private:
  PrjfsChannel* channel_;
  int32_t commandId_;
};

} // namespace facebook::eden
