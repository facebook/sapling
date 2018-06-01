/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <sys/types.h>
#include <limits>
#include <set>
#include <string>
#include <unordered_map>
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"

namespace folly {
class File;
}

namespace facebook {
namespace eden {

/*
 * PrivHelperServer runs the main loop for the privhelper server process.
 *
 * This processes requests received on the specified socket.
 * The server exits when the remote side of the socket is closed.
 *
 * See PrivHelperConn.h for the various message types.
 *
 * The uid and gid parameters specify the user and group ID of the unprivileged
 * process that will be making requests to us.
 */
class PrivHelperServer {
 public:
  PrivHelperServer();
  virtual ~PrivHelperServer();

  void init(PrivHelperConn&& conn, uid_t uid, gid_t gid);
  void run();

 private:
  void initLogging();

  [[noreturn]] void messageLoop();
  void cleanupMountPoints();
  void processMountMsg(PrivHelperConn::Message* msg);
  void processUnmountMsg(PrivHelperConn::Message* msg);
  void processBindMountMsg(PrivHelperConn::Message* msg);
  void processTakeoverShutdownMsg(PrivHelperConn::Message* msg);
  void processTakeoverStartupMsg(PrivHelperConn::Message* msg);

  // These methods are virtual so we can override them during unit tests
  virtual folly::File fuseMount(const char* mountPath);
  virtual void fuseUnmount(const char* mountPath);
  // Both clientPath and mountPath must be existing directories.
  virtual void bindMount(const char* clientPath, const char* mountPath);
  virtual void bindUnmount(const char* mountPath);

  PrivHelperConn conn_;
  uid_t uid_{std::numeric_limits<uid_t>::max()};
  gid_t gid_{std::numeric_limits<gid_t>::max()};

  // The privhelper server only has a single thread,
  // so we don't need to lock the following state
  std::set<std::string> mountPoints_;
  std::unordered_multimap<std::string, std::string> bindMountPoints_;
};

} // namespace eden
} // namespace facebook
