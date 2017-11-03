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

#include <folly/Range.h>
#include <sys/types.h>

namespace folly {
class File;
}

namespace facebook {
namespace eden {

class UserInfo;

namespace fusell {

class PrivHelperServer;

/*
 * Fork a separate privileged helper process, for performing mounts.
 *
 * This function should be called once, very early on during program
 * initialization, before any other threads are forked.  After it is called
 * UserInfo::dropPrivileges() should be called to return the desired user
 * privileges.
 */
void startPrivHelper(const UserInfo& userInfo);

/*
 * Start the privhelper process using a custom PrivHelperServer class.
 *
 * This is really only intended for use in unit tests.
 */
void startPrivHelper(PrivHelperServer* server, const UserInfo& userInfo);

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
 * Throws an exception if the privhelper was not running, or if any other error
 * occurs.
 */
int stopPrivHelper();

/*
 * Ask the privileged helper process to perform a fuse mount.
 *
 * Returns a folly::File object with the file descriptor containing the fuse
 * connection.  Throws an exception on error.
 *
 * The mountFlags and mountOpts parameters here are passed to the mount(2)
 * system call.
 * TODO(simpkins): I'm just going to drop these arguments, so that the
 * unprivileged process doesn't have control of them.  The privhelper process
 * itself will just pick the right values.
 */
folly::File privilegedFuseMount(folly::StringPiece mountPath);

/*
 * Ask the priveleged helper process to perform a fuse unmount. Throws an
 * exception on error.
 */
void privilegedFuseUnmount(folly::StringPiece mountPath);

/*
 * @param clientPath Absolute path (that should be under
 *     .eden/clients/<client-name>/bind-mounts/) where the "real" storage is.
 * @param mountPath Absolute path where the bind mount should be applied.
 */
void privilegedBindMount(
    folly::StringPiece clientPath,
    folly::StringPiece mountPath);
} // namespace fusell
} // namespace eden
} // namespace facebook
