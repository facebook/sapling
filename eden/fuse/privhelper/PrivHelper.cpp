/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "PrivHelper.h"

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/String.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#include <mutex>

#include "PrivHelperConn.h"
#include "PrivHelperServer.h"

using folly::checkUnixError;
using std::string;

namespace facebook {
namespace eden {
namespace fusell {

namespace {

/*
 * PrivHelper contains the client-side logic (in the parent process)
 * for talking to the remote privileged process.
 */
class PrivHelper {
 public:
  PrivHelper(PrivHelperConn&& conn, pid_t helperPid, uid_t uid, gid_t gid)
      : conn_(std::move(conn)), helperPid_(helperPid), uid_(uid), gid_(gid) {}
  ~PrivHelper() {
    // Close the socket.  This signals the privhelper process to exit.
    conn_.close();

    // Wait until the privhelper process exits.
    int status;
    pid_t pid;
    do {
      pid = waitpid(helperPid_, &status, 0);
    } while (pid == -1 && errno == EINTR);
    if (pid == -1) {
      LOG(ERROR) << "error waiting on privhelper process: "
                 << folly::errnoStr(errno);
    }
  }

  /*
   * Drop priviliges down to those requested when creating the PrivHelper
   */
  void dropPrivileges() {
    int rc = setregid(gid_, gid_);
    checkUnixError(rc, "failed to drop group privileges");
    rc = setreuid(uid_, uid_);
    checkUnixError(rc, "failed to drop user privileges");
  }

  /*
   * Send a request then receive the response.
   *
   * The response is placed into the same message buffer used for the request.
   */
  void sendAndRecv(PrivHelperConn::Message* msg, folly::File* fd) {
    // Hold the lock.
    // We only support a single operation at a time for now.
    // (The privhelper process only has a single thread anyway, and we don't
    // currently support processing out-of-order responses.)
    std::lock_guard<std::mutex> guard(mutex_);

    auto requestXid = nextXid_;
    ++nextXid_;
    msg->xid = requestXid;
    conn_.sendMsg(msg);
    conn_.recvMsg(msg, fd);

    if (msg->xid != requestXid) {
      throw std::runtime_error(folly::to<string>(
          "mismatched privhelper response: request XID was ",
          requestXid,
          "; got response XID ",
          msg->xid));
    }
  }

 private:
  std::mutex mutex_;
  PrivHelperConn conn_;
  const pid_t helperPid_{0};
  const uid_t uid_{0};
  const gid_t gid_{0};
  uint32_t nextXid_{1};
};

// The global PrivHelper for use in the parent (non-privileged) process
std::unique_ptr<PrivHelper> gPrivHelper;

} // unnamed namespace

void startPrivHelper(uid_t uid, gid_t gid) {
  CHECK_EQ(geteuid(), 0) << "must be root in order to start the privhelper";
  PrivHelperServer server;
  startPrivHelper(&server, uid, gid);
}

void startPrivHelper(PrivHelperServer* server, uid_t uid, gid_t gid) {
  CHECK(!gPrivHelper) << "privhelper already initialized";

  PrivHelperConn clientConn;
  PrivHelperConn serverConn;
  PrivHelperConn::createConnPair(clientConn, serverConn);

  auto pid = fork();
  checkUnixError(pid, "failed to fork mount helper");
  if (pid > 0) {
    // Parent
    serverConn.close();
    gPrivHelper.reset(new PrivHelper(std::move(clientConn), pid, uid, gid));
    VLOG(1) << "Forked mount helper process: pid=" << pid;
    return;
  }

  // Child
  clientConn.close();
  int rc = 1;
  try {
    server->init(std::move(serverConn), uid, gid);
    server->run();
    rc = 0;
  } catch (const std::exception& ex) {
    LOG(ERROR) << "error inside mount helper: " << folly::exceptionStr(ex);
  } catch (...) {
    LOG(ERROR) << "invalid type thrown inside mount helper";
  }
  _exit(rc);
}

void stopPrivHelper() {
  gPrivHelper.reset();
}

void dropPrivileges() {
  gPrivHelper->dropPrivileges();
}

folly::File privilegedFuseMount(folly::StringPiece mountPath) {
  PrivHelperConn::Message msg;
  PrivHelperConn::serializeMountRequest(&msg, mountPath);

  folly::File file;
  gPrivHelper->sendAndRecv(&msg, &file);
  PrivHelperConn::parseEmptyResponse(&msg);
  CHECK(file) << "no file descriptor received in privhelper mount response";
  return file;
}

void privilegedFuseUnmount(folly::StringPiece mountPath) {
  PrivHelperConn::Message msg;
  PrivHelperConn::serializeUnmountRequest(&msg, mountPath);

  gPrivHelper->sendAndRecv(&msg, nullptr);
  PrivHelperConn::parseEmptyResponse(&msg);
}

void privilegedBindMount(
    folly::StringPiece clientPath,
    folly::StringPiece mountPath) {
  PrivHelperConn::Message msg;
  PrivHelperConn::serializeBindMountRequest(&msg, clientPath, mountPath);

  gPrivHelper->sendAndRecv(&msg, nullptr);
  PrivHelperConn::parseEmptyResponse(&msg);
}
}
}
} // facebook::eden::fusell
