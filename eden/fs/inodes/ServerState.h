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

#include "eden/fs/fuse/EdenStats.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class PrivHelper;

/**
 * ServerState contains state shared across multiple mounts.
 *
 * This is normally owned by the main EdenServer object.  However unit tests
 * also create ServerState objects without an EdenServer.
 */
class ServerState {
 public:
  ServerState();
  ServerState(UserInfo userInfo, std::shared_ptr<PrivHelper> privHelper);
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
  fusell::ThreadLocalEdenStats& getStats() {
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

 private:
  AbsolutePath socketPath_;
  UserInfo userInfo_;
  fusell::ThreadLocalEdenStats edenStats_;
  std::shared_ptr<PrivHelper> privHelper_;
};
} // namespace eden
} // namespace facebook
