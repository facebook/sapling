/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/privhelper/PrivHelper.h"

#include <folly/Exception.h>
#include <folly/Expected.h>
#include <folly/File.h>
#include <folly/String.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#include <mutex>

#include "eden/fs/fuse/privhelper/PrivHelperConn.h"
#include "eden/fs/fuse/privhelper/PrivHelperServer.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"

using folly::checkUnixError;
using folly::File;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Unit;
using folly::unit;
using std::make_unique;
using std::string;
using std::unique_ptr;
using std::vector;

namespace facebook {
namespace eden {

namespace {

/**
 * PrivHelperClientImpl contains the client-side logic (in the parent process)
 * for talking to the remote privileged process.
 */
class PrivHelperClientImpl : public PrivHelper {
 public:
  PrivHelperClientImpl(PrivHelperConn&& conn, pid_t helperPid)
      : conn_(std::move(conn)), helperPid_(helperPid) {}
  ~PrivHelperClientImpl() {
    if (!conn_.isClosed()) {
      cleanup();
    }
  }

  Future<File> fuseMount(folly::StringPiece mountPath) override;
  Future<Unit> fuseUnmount(StringPiece mountPath) override;
  Future<Unit> bindMount(StringPiece clientPath, StringPiece mountPath)
      override;
  Future<Unit> fuseTakeoverShutdown(StringPiece mountPath) override;
  Future<Unit> fuseTakeoverStartup(
      StringPiece mountPath,
      const vector<string>& bindMounts) override;
  int stop() override;

 private:
  /**
   * Close the socket to the privhelper server, and wait for it to exit.
   *
   * Returns the exit status of the privhelper process, or an errno value on
   * error.
   */
  folly::Expected<int, int> cleanup() {
    if (conn_.isClosed()) {
      // The privhelper process was already closed
      return folly::makeUnexpected(ESRCH);
    }

    // Close the socket.  This signals the privhelper process to exit.
    conn_.close();

    // Wait until the privhelper process exits.
    int status;
    pid_t pid;
    do {
      pid = waitpid(helperPid_, &status, 0);
    } while (pid == -1 && errno == EINTR);
    if (pid == -1) {
      XLOG(ERR) << "error waiting on privhelper process: "
                << folly::errnoStr(errno);
      return folly::makeUnexpected(errno);
    }
    if (WIFSIGNALED(status)) {
      return folly::makeExpected<int>(-WTERMSIG(status));
    }
    DCHECK(WIFEXITED(status)) << "unexpected exit status type: " << status;
    return folly::makeExpected<int>(WEXITSTATUS(status));
  }

  /**
   * Send a request then receive the response.
   *
   * The response is placed into the same message buffer used for the request.
   */
  void sendAndRecv(PrivHelperConn::Message* msg, folly::File* fd) {
    // Hold the lock.
    // We only support a single operation at a time for now.
    // (The privhelper process only has a single thread anyway, and we don't
    // currently support processing out-of-order responses.)
    const std::lock_guard<std::mutex> guard(mutex_);

    const auto requestXid = nextXid_;
    ++nextXid_;
    msg->xid = requestXid;

    // Send the message
    conn_.sendMsg(msg);

    // Receive the response
    size_t numRetries = 0;
    while (true) {
      conn_.recvMsg(msg, fd);
      if (msg->xid == requestXid) {
        break;
      }

      // If we timed out waiting for a response to a previous request
      // we might receive it now, before the response to our request.
      //
      // If the transaction ID looks like a fairly recent one, just
      // ignore it and try to receive another message.
      if (msg->xid < requestXid && msg->xid >= requestXid - 5 &&
          numRetries < 5) {
        XLOG(DBG1) << "ignoring stale privhelper response " << msg->xid
                   << " while waiting for " << requestXid;
        numRetries++;
        continue;
      }

      // Otherwise give up.
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
  uint32_t nextXid_{1};
};

Future<File> PrivHelperClientImpl::fuseMount(StringPiece mountPath) {
  PrivHelperConn::Message msg;
  PrivHelperConn::serializeMountRequest(&msg, mountPath);

  folly::File file;
  sendAndRecv(&msg, &file);
  PrivHelperConn::parseEmptyResponse(&msg);
  CHECK(file) << "no file descriptor received in privhelper mount response";
  return file;
}

Future<Unit> PrivHelperClientImpl::fuseUnmount(StringPiece mountPath) {
  PrivHelperConn::Message msg;
  PrivHelperConn::serializeUnmountRequest(&msg, mountPath);

  sendAndRecv(&msg, nullptr);
  PrivHelperConn::parseEmptyResponse(&msg);
  return unit;
}

Future<Unit> PrivHelperClientImpl::bindMount(
    StringPiece clientPath,
    StringPiece mountPath) {
  PrivHelperConn::Message msg;
  PrivHelperConn::serializeBindMountRequest(&msg, clientPath, mountPath);

  sendAndRecv(&msg, nullptr);
  PrivHelperConn::parseEmptyResponse(&msg);
  return unit;
}

Future<Unit> PrivHelperClientImpl::fuseTakeoverShutdown(StringPiece mountPath) {
  PrivHelperConn::Message msg;
  PrivHelperConn::serializeTakeoverShutdownRequest(&msg, mountPath);

  sendAndRecv(&msg, nullptr);
  PrivHelperConn::parseEmptyResponse(&msg);
  return unit;
}

Future<Unit> PrivHelperClientImpl::fuseTakeoverStartup(
    StringPiece mountPath,
    const vector<string>& bindMounts) {
  PrivHelperConn::Message msg;
  PrivHelperConn::serializeTakeoverStartupRequest(&msg, mountPath, bindMounts);

  sendAndRecv(&msg, nullptr);
  PrivHelperConn::parseEmptyResponse(&msg);
  return unit;
}

int PrivHelperClientImpl::stop() {
  if (conn_.isClosed()) {
    throw std::runtime_error(
        "attempted to stop the privhelper process when it was not running");
  }
  const auto result = cleanup();
  if (result.hasError()) {
    folly::throwSystemErrorExplicit(
        result.error(), "error shutting down privhelper process");
  }
  return result.value();
}

} // unnamed namespace

unique_ptr<PrivHelper> startPrivHelper(const UserInfo& userInfo) {
  CHECK_EQ(geteuid(), 0) << "must be root in order to start the privhelper";
  PrivHelperServer server;
  return startPrivHelper(&server, userInfo);
}

unique_ptr<PrivHelper> startPrivHelper(
    PrivHelperServer* server,
    const UserInfo& userInfo) {
  PrivHelperConn clientConn;
  PrivHelperConn serverConn;
  PrivHelperConn::createConnPair(clientConn, serverConn);

  const auto pid = fork();
  checkUnixError(pid, "failed to fork mount helper");
  if (pid > 0) {
    // Parent
    serverConn.close();
    XLOG(DBG1) << "Forked mount helper process: pid=" << pid;
    return make_unique<PrivHelperClientImpl>(std::move(clientConn), pid);
  }

  // Child
  clientConn.close();
  int rc = 1;
  try {
    server->init(std::move(serverConn), userInfo.getUid(), userInfo.getGid());
    server->run();
    rc = 0;
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error inside mount helper: " << folly::exceptionStr(ex);
  } catch (...) {
    XLOG(ERR) << "invalid type thrown inside mount helper";
  }
  _exit(rc);
}

} // namespace eden
} // namespace facebook
