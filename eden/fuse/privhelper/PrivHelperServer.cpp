/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "PrivHelperServer.h"

#include <fcntl.h>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Format.h>
#include <folly/String.h>
#include <signal.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <set>

#include "PrivHelperConn.h"

using folly::checkUnixError;
using folly::throwSystemError;
using std::string;

namespace facebook {
namespace eden {
namespace fusell {

PrivHelperServer::PrivHelperServer() {}

PrivHelperServer::~PrivHelperServer() {}

void PrivHelperServer::init(PrivHelperConn&& conn, uid_t uid, gid_t gid) {
  // Make sure init() is only called once.
  CHECK_EQ(uid_, std::numeric_limits<uid_t>::max());
  CHECK_EQ(gid_, std::numeric_limits<gid_t>::max());

  conn_ = std::move(conn);
  uid_ = uid;
  gid_ = gid;
}

folly::File PrivHelperServer::fuseMount(const char* mountPath) {
  // We manually call open() here rather than using the folly::File()
  // constructor just so we can emit a slightly more helpful message on error.
  const char* devName = "/dev/fuse";
  int fd = folly::openNoInt(devName, O_RDWR);
  if (fd < 0) {
    if (errno == ENODEV || errno == ENOENT) {
      throwSystemError(
          "failed to open ",
          devName,
          ": make sure the fuse kernel module is loaded");
    } else {
      throwSystemError("failed to open ", devName);
    }
  }
  folly::File fuseDev(fd, true);

  // Prepare the flags and options to pass to mount(2).
  // We currently don't allow these to be customized by the unprivileged
  // requester.  We could add this functionality in the future if we have a
  // need for it, but we would need to validate their changes are safe.
  int rootMode = S_IFDIR;
  auto mountOpts = folly::sformat(
      "allow_other,rootmode={:o},user_id={},group_id={},fd={}",
      rootMode,
      uid_,
      gid_,
      fuseDev.fd());

  // The mount flags.
  // We do not use MS_NODEV.  MS_NODEV prevents mount points from being created
  // inside our filesystem.  We currently use bind mounts to point the buck-out
  // directory to an alternate location outside of eden.
  int mountFlags = MS_NOSUID;

  const char* type = "fuse";
  int rc = mount("edenfs", mountPath, type, mountFlags, mountOpts.c_str());
  checkUnixError(rc, "failed to mount");
  return fuseDev;
}

void PrivHelperServer::fuseUnmount(const char* mountPath) {
  auto rc = umount2(mountPath, UMOUNT_NOFOLLOW);
  if (rc != 0) {
    int errnum = errno;
    // EINVAL simply means the path is no longer mounted.
    // This can happen if it was already manually unmounted by a
    // separate process.
    if (errnum != EINVAL) {
      LOG(WARNING) << "error unmounting " << mountPath << ": "
                   << folly::errnoStr(errnum);
    }
  }
}

void PrivHelperServer::processMountMsg(PrivHelperConn::Message* msg) {
  string mountPath;
  conn_.parseMountRequest(msg, mountPath);

  folly::File fuseDev;
  try {
    fuseDev = fuseMount(mountPath.c_str());
    mountPoints_.insert(mountPath);
    conn_.serializeMountResponse(msg);
  } catch (const std::exception& ex) {
    // Note that we re-use the request message buffer for the response data
    conn_.serializeErrorResponse(msg, ex);
    conn_.sendMsg(msg);
    return;
  }

  // Note that we re-use the request message buffer for the response data
  conn_.sendMsg(msg, fuseDev.fd());
}

void PrivHelperServer::messageLoop() {
  PrivHelperConn::Message msg;

  while (1) {
    conn_.recvMsg(&msg, nullptr);
    if (msg.msgType == PrivHelperConn::REQ_MOUNT) {
      processMountMsg(&msg);
    } else {
      // This shouldn't ever happen unless we have a bug.
      // Crash if it does occur.  (We could send back an error message and
      // continue, but it seems better to fail hard to make sure this bug gets
      // noticed and debugged.)
      LOG(FATAL) << "unsupported privhelper message type: " << msg.msgType;
    }
  }
}

void PrivHelperServer::cleanupMountPoints() {
  for (const auto& mp : mountPoints_) {
    fuseUnmount(mp.c_str());
  }
  mountPoints_.clear();
}

void PrivHelperServer::run() {
  // Ignore SIGINT and SIGTERM.
  // We should only exit when our parent process does.
  // (Normally if someone hits Ctrl-C in their terminal this will send SIGINT
  // to both our parent process and to us.  The parent process should exit due
  // to this signal.  We don't want to exit immediately--we want to wait until
  // the parent exits and then umount all outstanding mount points before we
  // exit.)
  auto sigret = signal(SIGINT, SIG_IGN);
  if (sigret == SIG_ERR) {
    LOG(FATAL) << "error setting SIGINT handler in privhelper process"
               << folly::errnoStr(errno);
  }
  sigret = signal(SIGTERM, SIG_IGN);
  if (sigret == SIG_ERR) {
    LOG(FATAL) << "error setting SIGTERM handler in privhelper process"
               << folly::errnoStr(errno);
  }

  try {
    messageLoop();
  } catch (const PrivHelperClosedError& ex) {
    // The parent process exited, so we can quit too.
    VLOG(5) << "privhelper process exiting";
  }

  // Unmount all active mount points
  cleanupMountPoints();
}
}
}
} // facebook::eden::fusell
