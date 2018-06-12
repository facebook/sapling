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

#include <folly/Portability.h>
#include <folly/Range.h>
#include <sys/types.h>
#include <memory>

namespace folly {
class File;
template <typename T>
class Future;
class Unit;
}

namespace facebook {
namespace eden {

class UserInfo;

/**
 * A helper class for performing operations that require elevated privileges.
 *
 * This sends the requests to a separate helper process that has the privileges
 * required to peform these operations.
 */
class PrivHelper {
 public:
  virtual ~PrivHelper() {}

  /**
   * Ask the privileged helper process to perform a fuse mount.
   *
   * Returns a folly::File object with the file descriptor containing the fuse
   * connection.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::File> fuseMount(
      folly::StringPiece mountPath) = 0;

  /**
   * Ask the priveleged helper process to perform a fuse unmount.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> fuseUnmount(
      folly::StringPiece mountPath) = 0;

  /**
   * @param clientPath Absolute path (that should be under
   *     .eden/clients/<client-name>/bind-mounts/) where the "real" storage is.
   * @param mountPath Absolute path where the bind mount should be applied.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> bindMount(
      folly::StringPiece clientPath,
      folly::StringPiece mountPath) = 0;

  /**
   * Inform the privhelper process that we are shutting down as part of a
   * graceful restart, and a new edenfs daemon will take over our existing
   * mount points without unmounting them.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> fuseTakeoverShutdown(
      folly::StringPiece mountPath) = 0;

  /**
   * Inform the privhelper process that we have taken over an existing mount
   * point from another edenfs process.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> fuseTakeoverStartup(
      folly::StringPiece mountPath,
      const std::vector<std::string>& bindMounts) = 0;

  /*
   * Explicitly stop the privhelper process.
   *
   * The privhelper process will exit automatically when the main process exits
   * even if this method is not called.  However, this method can be used to
   * explictly stop the privhelper process, and check its exit code.
   *
   * Note that when the privhelper is stopped it will unmount any outstanding
   * mounts points.
   *
   * If the privhelper exited normally, the exit code is returned.
   * If the privhelper was terminated due to a signal, the signal number is
   * returned as a negative number.
   *
   * Throws an exception if the privhelper was not running, or if any other
   * error occurs.
   */
  virtual int stop() = 0;
};

class PrivHelperServer;

/**
 * Fork a separate privileged helper process, for performing mounts.
 *
 * This function should be very early on during program initialization, before
 * any other threads are forked.  After it is called UserInfo::dropPrivileges()
 * should be called to return the desired user privileges.
 */
std::unique_ptr<PrivHelper> startPrivHelper(const UserInfo& userInfo);

/**
 * Start a privhelper process using a custom PrivHelperServer class.
 *
 * This is really only intended for use in unit tests.
 */
std::unique_ptr<PrivHelper> startPrivHelper(
    PrivHelperServer* server,
    const UserInfo& userInfo);

} // namespace eden
} // namespace facebook
