/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h> // @manual
#include "eden/fs/prjfs/FsChannel.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/ProcessAccessLog.h"

namespace facebook {
namespace eden {
class EdenMount;
class Dispatcher;

class PrjfsChannel : public FsChannel {
 public:
  PrjfsChannel(const PrjfsChannel&) = delete;
  PrjfsChannel& operator=(const PrjfsChannel&) = delete;

  explicit PrjfsChannel() = delete;

  PrjfsChannel(
      AbsolutePathPiece mountPath,
      Dispatcher* const dispatcher,
      std::shared_ptr<ProcessNameCache> processNameCache);
  ~PrjfsChannel();

  void start(bool readOnly, bool useNegativePathCaching);
  void stop();

  folly::SemiFuture<FsChannel::StopData> getStopFuture() override;

  /**
   * Remove files from the Projected FS cache. removeCachedFile() doesn't care
   * about the file state and will remove file in any state.
   */
  void removeCachedFile(RelativePathPiece path) override;

  void addDirectoryPlaceholder(RelativePathPiece path) override;

  void flushNegativePathCache() override;

  Dispatcher* getDispatcher() {
    return dispatcher_;
  }

  ProcessAccessLog& getProcessAccessLog() {
    return processAccessLog_;
  }

  PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT getMountChannelContext() const {
    return mountChannel_;
  }

  void sendSuccess(
      int32_t commandId,
      PRJ_COMPLETE_COMMAND_EXTENDED_PARAMETERS* FOLLY_NULLABLE extra);

  void sendError(int32_t commandId, HRESULT error);

 private:
  //
  // Channel to talk to projectedFS.
  //
  PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT mountChannel_{nullptr};

  const AbsolutePath mountPath_;
  Dispatcher* const dispatcher_{nullptr};
  Guid mountId_;
  bool isRunning_{false};
  bool useNegativePathCaching_{true};
  folly::Promise<FsChannel::StopData> stopPromise_;

  ProcessAccessLog processAccessLog_;
};

} // namespace eden
} // namespace facebook
