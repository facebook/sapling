/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/takeover/TakeoverServer.h"

#include <chrono>

#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/EventHandler.h>
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverHandler.h"
#include "eden/fs/utils/ControlMsg.h"
#include "eden/fs/utils/IoFuture.h"

using namespace std::literals::chrono_literals;

using folly::AsyncServerSocket;
using folly::Future;
using folly::SocketAddress;
using folly::StringPiece;
using folly::Unit;
using folly::checkUnixError;
using folly::exceptionStr;
using folly::makeFuture;
using std::make_unique;

namespace facebook {
namespace eden {

/**
 * ConnHandler handles a single connection received on the TakeoverServer
 * socket.
 */
class TakeoverServer::ConnHandler {
 public:
  ConnHandler(TakeoverServer* server, int fd)
      : server_{server},
        socket_{fd},
        ioFuture_{server_->getEventBase(), socket_} {}

  ~ConnHandler() {
    folly::closeNoInt(socket_);
  }

  /**
   * start() begins processing data on this connection.
   *
   * Returns a Future that will complete successfully when this connection
   * finishes gracefully taking over the EdenServer's mount points.
   */
  folly::Future<folly::Unit> start();

 private:
  folly::Future<folly::Unit> checkCredentials();
  folly::Future<folly::Unit> sendNormalData();
  folly::Future<folly::Unit> sendFDs();

  folly::Future<int> waitForIO(
      int eventFlags,
      std::chrono::milliseconds timeout) {
    return ioFuture_.wait(eventFlags, timeout);
  }

  template <typename... Args>
  [[noreturn]] void fail(Args&&... args) {
    auto msg = folly::to<std::string>(std::forward<Args>(args)...);
    XLOG(ERR) << "takeover socket error: " << msg;
    throw std::runtime_error(msg);
  }

  TakeoverServer* server_{nullptr};
  int socket_{-1};
  IoFuture ioFuture_;
  TakeoverData takeoverData_;
  size_t fdIndex_{0};
  std::unique_ptr<folly::IOBuf> dataBuf_;
  folly::fbvector<struct iovec> dataIovec_;
  size_t dataIovIndex_{0};
};

Future<Unit> TakeoverServer::ConnHandler::start() {
  // Enable SO_PASSCRED so we can receive credentials
  int opt = 1;
  int rc = setsockopt(socket_, SOL_SOCKET, SO_PASSCRED, &opt, sizeof(opt));
  checkUnixError(rc, "error enabling SO_PASSCRED on takeover connection");

  return waitForIO(folly::EventHandler::READ, 10s)
      .then([this] { return checkCredentials(); })
      .then([this] {
        return server_->getTakeoverHandler()->startTakeoverShutdown();
      })
      .then([this](folly::Try<TakeoverData>&& data) {
        if (data.hasValue()) {
          // Save takeover data
          takeoverData_ = std::move(data.value());
          // Serialize the non-FD data into a buffer
          dataBuf_ = takeoverData_.serialize();
          dataIovec_ = dataBuf_->getIov();
          return sendNormalData()
              .then([this] {
                // Send the mount FDs after we finish sending the mount paths
                return sendFDs();
              })
              .then([this] { takeoverData_.takeoverComplete.setValue(); })
              .onError([this](folly::exception_wrapper&& ew) {
                takeoverData_.takeoverComplete.setException(std::move(ew));
              });
        } else {
          // Serialize the error data
          dataBuf_ = TakeoverData::serializeError(data.exception());
          dataIovec_ = dataBuf_->getIov();
          dataIovIndex_ = 0;
          return sendNormalData();
        }
      });
}

Future<Unit> TakeoverServer::ConnHandler::checkCredentials() {
  // Set up a ControlMsgBuffer to provide space to receive credentials.
  ControlMsgBuffer cmsgBuf(sizeof(struct ucred), SOL_SOCKET, SCM_CREDENTIALS);

  // We only expect clients to send 1 data byte
  struct iovec iov;
  uint8_t dataByte = 0;
  iov.iov_base = &dataByte;
  iov.iov_len = sizeof(dataByte);

  struct msghdr msg;
  msg.msg_name = nullptr;
  msg.msg_namelen = 0;
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;
  cmsgBuf.addToMsg(&msg);
  msg.msg_flags = 0;

  auto bytesReceived = recvmsg(socket_, &msg, MSG_DONTWAIT);
  checkUnixError(bytesReceived, "recvmsg() failed on takeover socket");

  ControlMsg recvCmsg = ControlMsg::fromMsg(
      msg, SOL_SOCKET, SCM_CREDENTIALS, sizeof(struct ucred));
  auto* cred = recvCmsg.getData<struct ucred>();
  XLOG(INFO) << "received takeover request from UID=" << cred->uid
             << " GID=" << cred->gid << " PID=" << cred->pid;

  // Check that the UID matches the UID we are currently running as.
  //
  // We intentionally don't check the GID for now; it seems worth allowing the
  // user to restart even if their primary GID has changed for some reason.
  if (cred->uid != getuid()) {
    fail(
        "invalid takeover request from incorrect user: current UID=",
        getuid(),
        ", got request from UID ",
        cred->uid);
  }

  return makeFuture();
}

folly::Future<folly::Unit> TakeoverServer::ConnHandler::sendNormalData() {
  while (dataIovIndex_ < dataIovec_.size()) {
    struct msghdr msg;
    msg.msg_name = nullptr;
    msg.msg_namelen = 0;
    msg.msg_iov = dataIovec_.data() + dataIovIndex_;
    msg.msg_iovlen = dataIovec_.size() - dataIovIndex_;
    msg.msg_control = nullptr;
    msg.msg_controllen = 0;
    msg.msg_flags = 0;

    // Now call sendmsg
    auto bytesSent = sendmsg(socket_, &msg, MSG_DONTWAIT);
    if (bytesSent < 0) {
      if (errno == EAGAIN) {
        return waitForIO(folly::EventHandler::WRITE, 5s).then([this] {
          return sendNormalData();
        });
      }
      folly::throwSystemError("error sending takeover mount paths");
    }

    // Update dataIovec_ and dataIovIndex_ to account
    // for the data that was successfully sent.
    while (bytesSent > 0) {
      if (bytesSent >= dataIovec_[dataIovIndex_].iov_len) {
        bytesSent -= dataIovec_[dataIovIndex_].iov_len;
        ++dataIovIndex_;
      } else {
        auto* iov = &dataIovec_[dataIovIndex_];
        iov->iov_len -= bytesSent;
        iov->iov_base = static_cast<char*>(iov->iov_base) + bytesSent;
        break;
      }
    }
  }

  // We successfully sent all of the mount paths
  XLOG(DBG4) << "successfully transferred mount point paths";
  return makeFuture();
}

folly::Future<folly::Unit> TakeoverServer::ConnHandler::sendFDs() {
  // We need to send all of the mount point FDs,
  // plus the lock file and the thrift socket.
  auto totalFDs = takeoverData_.mountPoints.size() + 2;

  while (true) {
    auto fdsLeft = totalFDs - fdIndex_;
    if (fdsLeft == 0) {
      break;
    }

    // Limit the number of FDs in one message to ControlMsg::kMaxFDs
    auto fdsToSend = std::min(ControlMsg::kMaxFDs, fdsLeft);
    ControlMsgBuffer cmsg(fdsToSend * sizeof(int), SOL_SOCKET, SCM_RIGHTS);

    // Put the FDs into the message
    auto* fds = cmsg.getData<int>();
    for (size_t n = 0; n < fdsToSend; ++n) {
      auto mountIndex = fdIndex_ + n;
      if (mountIndex < takeoverData_.mountPoints.size()) {
        fds[n] = takeoverData_.mountPoints[mountIndex].fuseFD.fd();
      } else if (mountIndex == takeoverData_.mountPoints.size()) {
        fds[n] = takeoverData_.lockFile.fd();
      } else {
        CHECK_EQ(mountIndex, takeoverData_.mountPoints.size() + 1);
        fds[n] = takeoverData_.thriftSocket.fd();
      }
    }

    // Send the message
    uint8_t data = 0;
    struct iovec iov;
    iov.iov_base = &data;
    iov.iov_len = sizeof(data);

    struct msghdr msg;
    msg.msg_name = nullptr;
    msg.msg_namelen = 0;
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    cmsg.addToMsg(&msg);
    msg.msg_flags = 0;

    auto bytesSent = sendmsg(socket_, &msg, MSG_DONTWAIT);
    if (bytesSent < 0) {
      if (errno == EAGAIN) {
        return waitForIO(folly::EventHandler::WRITE, 5s).then([this] {
          return sendFDs();
        });
      }
      folly::throwSystemError("error sending takeover mount FDs");
    }

    // Advance fdIndex_ over the FDs that we have successfully sent.
    fdIndex_ += fdsToSend;
  }

  // We've finished
  XLOG(DBG4) << "successfully transferred mount point file descriptors";
  return makeFuture();
}

TakeoverServer::TakeoverServer(
    folly::EventBase* eventBase,
    AbsolutePathPiece socketPath,
    TakeoverHandler* handler)
    : eventBase_{eventBase}, handler_{handler}, socketPath_{socketPath} {
  start();
}

TakeoverServer::~TakeoverServer() {}

void TakeoverServer::start() {
  // Build the address for the takeover socket.
  SocketAddress address;
  address.setFromPath(socketPath_.stringPiece());

  // Remove any old file at this path, so we can bind to it.
  auto rc = unlink(socketPath_.value().c_str());
  if (rc != 0 && errno != ENOENT) {
    folly::throwSystemError("error removing old takeover socket");
  }

  socket_.reset(new AsyncServerSocket{eventBase_});
  socket_->bind(address);
  socket_->listen(/* backlog */ 1024);
  socket_->addAcceptCallback(this, nullptr);
  socket_->startAccepting();
}

void TakeoverServer::connectionAccepted(
    int fd,
    const folly::SocketAddress& /* clientAddr */) noexcept {
  std::unique_ptr<ConnHandler> handler;
  try {
    handler.reset(new ConnHandler{this, fd});
  } catch (const std::exception& ex) {
    folly::closeNoInt(fd);
    XLOG(ERR) << "error allocating connection handler for new takeover "
                 "connection: "
              << exceptionStr(ex);
    return;
  }

  XLOG(INFO) << "takeover socket connection received";
  auto* handlerRawPtr = handler.get();
  handlerRawPtr->start()
      .onError([](const folly::exception_wrapper& ew) {
        XLOG(ERR) << "error processing takeover connection request: "
                  << folly::exceptionStr(ew);
      })
      .ensure([h = std::move(handler)] {});
}

void TakeoverServer::acceptError(const std::exception& ex) noexcept {
  XLOG(ERR) << "accept() error on takeover socket: " << exceptionStr(ex);
}
} // namespace eden
} // namespace facebook
