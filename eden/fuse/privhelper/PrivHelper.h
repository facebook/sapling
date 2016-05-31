/*
 *  Copyright (c) 2016, Facebook, Inc.
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
namespace fusell {

class PrivHelperServer;

/*
 * Fork a separate privileged helper process, for performing mounts.
 *
 * This function should be called once, very early on during program
 * initialization, before any other threads are forked.  After it is called
 * dropPrivileges() should be called to return the desired user privileges.
 */
void startPrivHelper(uid_t uid, gid_t gid);

/*
 * Start the privhelper process using a custom PrivHelperServer class.
 *
 * This is really only intended for use in unit tests.
 */
void startPrivHelper(PrivHelperServer* server, uid_t uid, gid_t gid);

/*
 * Explicitly stop the privhelper process.
 *
 * Normally you don't need to call this.   The privhelper process will
 * exit automatically when the main process exits.   This method is primarly
 * provided for exercising the privhelper server in unit tests.
 *
 * Note that when the privhelper is stopped it will unmount any outstanding
 * mounts points.
 */
void stopPrivHelper();

/*
 * Drop privileges down to the UID and GID requested when
 * startPrivHelper() was called.
 *
 * This should also be called early on during program initialization,
 * after startPrivHelper() and any other operations that need to be done
 * while the process is still privileged.
 */
void dropPrivileges();

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
}
}
} // facebook::eden::fusell
