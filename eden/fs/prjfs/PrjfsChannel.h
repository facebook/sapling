/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h> // @manual
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/ProcessAccessLog.h"
#include "folly/futures/Future.h"

namespace facebook {
namespace eden {
class EdenMount;
class Dispatcher;

class PrjfsChannel {
 public:
  PrjfsChannel(const PrjfsChannel&) = delete;
  PrjfsChannel& operator=(const PrjfsChannel&) = delete;

  explicit PrjfsChannel() = delete;

  PrjfsChannel(
      AbsolutePathPiece mountPath,
      Dispatcher* const dispatcher,
      const folly::Logger* straceLogger,
      std::shared_ptr<ProcessNameCache> processNameCache);
  ~PrjfsChannel();

  void start(bool readOnly, bool useNegativePathCaching);
  void stop();

  struct StopData {};
  folly::SemiFuture<StopData> getStopFuture();

  /**
   * Remove a file that has been cached on disk by ProjectedFS. This should be
   * called when the content of a materialized file has changed, typically
   * called during on an `update` operation.
   *
   * This can fail when the underlying file cannot be evicted from ProjectedFS,
   * one example is when the user has locked the file.
   */
  FOLLY_NODISCARD folly::Try<void> removeCachedFile(RelativePathPiece path);

  /**
   * Ensure that the directory is a placeholder so that ProjectedFS will always
   * invoke the opendir/readdir callbacks when the user is listing files in it.
   * This particularly matters for directories that were created by the user to
   * later be committed.
   */
  FOLLY_NODISCARD folly::Try<void> addDirectoryPlaceholder(
      RelativePathPiece path);

  void flushNegativePathCache();

  Dispatcher* getDispatcher() {
    return dispatcher_;
  }

  const folly::Logger& getStraceLogger() const {
    return *straceLogger_;
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
  const folly::Logger* const straceLogger_{nullptr};
  Guid mountId_;
  bool isRunning_{false};
  bool useNegativePathCaching_{true};
  folly::Promise<StopData> stopPromise_;

  ProcessAccessLog processAccessLog_;
};

} // namespace eden
} // namespace facebook
