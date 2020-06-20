/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>

namespace facebook {
namespace eden {

class FsChannel {
 public:
  FsChannel(const FsChannel&) = delete;
  FsChannel& operator=(const FsChannel&) = delete;

  FsChannel(){};
  virtual ~FsChannel() = default;
  virtual void start(AbsolutePath mountPath, bool readOnly) = 0;
  virtual void stop() = 0;

  virtual void removeCachedFile(RelativePathPiece path) = 0;
  virtual void removeDeletedFile(RelativePathPiece path) = 0;

  struct StopData {};
  virtual folly::SemiFuture<FsChannel::StopData> getStopFuture() = 0;
};

} // namespace eden
} // namespace facebook
//////////////////////////
