/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/ThreadLocal.h>
#include <memory>

#include "eden/fs/fuse/EdenStats.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class Clock;
class PrivHelper;
class UnboundedQueueThreadPool;

/**
 * ServerState contains state shared across multiple mounts.
 *
 * This is normally owned by the main EdenServer object.  However unit tests
 * also create ServerState objects without an EdenServer.
 */
class ServerState {
 public:
  ServerState(
      UserInfo userInfo,
      std::shared_ptr<PrivHelper> privHelper,
      std::shared_ptr<UnboundedQueueThreadPool> threadPool,
      std::shared_ptr<Clock> clock);
  ~ServerState();

  /**
   * Set the path to the server's thrift socket.
   *
   * This is called by EdenServer once it has initialized the thrift server.
   */
  void setSocketPath(AbsolutePathPiece path) {
    socketPath_ = path.copy();
  }

  /**
   * Get the path to the server's thrift socket.
   *
   * This is used by the EdenMount to populate the `.eden/socket` special file.
   */
  const AbsolutePath& getSocketPath() const {
    return socketPath_;
  }

  /**
   * Get the ThreadLocalEdenStats object that tracks process-wide (rather than
   * per-mount) statistics.
   */
  ThreadLocalEdenStats& getStats() {
    return edenStats_;
  }

  /**
   * Get the UserInfo object describing the user running this edenfs process.
   */
  const UserInfo& getUserInfo() const {
    return userInfo_;
  }

  /**
   * Get the PrivHelper object used to perform operations that require
   * elevated privileges.
   */
  PrivHelper* getPrivHelper() {
    return privHelper_.get();
  }

  /**
   * Get the thread pool.
   *
   * Adding new tasks to this thread pool executor will never block.
   */
  const std::shared_ptr<UnboundedQueueThreadPool>& getThreadPool() const {
    return threadPool_;
  }

  /**
   * Get the Clock.
   */
  const std::shared_ptr<Clock>& getClock() const {
    return clock_;
  }

 private:
  AbsolutePath socketPath_;
  UserInfo userInfo_;
  ThreadLocalEdenStats edenStats_;
  std::shared_ptr<PrivHelper> privHelper_;
  std::shared_ptr<UnboundedQueueThreadPool> threadPool_;
  std::shared_ptr<Clock> clock_;
};
} // namespace eden
} // namespace facebook
