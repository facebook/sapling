/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/takeover/TakeoverClient.h"

#include <folly/Exception.h>
#include <folly/SocketAddress.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/io/IOBuf.h>
#include <folly/portability/Fcntl.h>
#include <folly/portability/Sockets.h>
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/utils/ControlMsg.h"

using folly::IOBuf;
using folly::SocketAddress;
using std::string;

namespace facebook {
namespace eden {
namespace {

void sendCredentials(int fd) {
  ControlMsgBuffer cmsg(sizeof(struct ucred), SOL_SOCKET, SCM_CREDENTIALS);
  auto* cred = cmsg.getData<struct ucred>();
  cred->pid = getpid();
  cred->uid = getuid();
  cred->gid = getgid();

  // Linux requires that we include at least 1 byte of real (non-control) data
  // when sending a message.  Send a single 0 byte.
  struct iovec iov;
  uint8_t dataByte = 0;
  iov.iov_base = &dataByte;
  iov.iov_len = sizeof(dataByte);

  struct msghdr msg;
  msg.msg_name = nullptr;
  msg.msg_namelen = 0;
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;
  cmsg.addToMsg(&msg);
  msg.msg_flags = 0;

  int sendmsgFlags = 0;
  auto bytesSent = sendmsg(fd, &msg, sendmsgFlags);
  if (bytesSent != 1) {
    folly::throwSystemError("sendmsg() failed on takeover socket");
  }
}

void recvFull(int fd, void* buf, size_t len) {
  while (len > 0) {
    auto bytesRead = recv(fd, buf, len, 0);
    folly::checkUnixError(bytesRead, "receive error");
    if (bytesRead == 0) {
      throw std::runtime_error("received unexpected EOF");
    }
    len -= bytesRead;
    buf = static_cast<char*>(buf) + bytesRead;
  }
}

TakeoverData recvTakeoverData(int fd) {
  // Read the header
  std::array<uint8_t, TakeoverData::kHeaderLength> headerBuf;
  recvFull(fd, headerBuf.data(), headerBuf.size());
  IOBuf headerIobuf{IOBuf::WRAP_BUFFER, headerBuf.data(), headerBuf.size()};
  auto header = TakeoverData::deserializeHeader(&headerIobuf);

  // Read the body
  IOBuf bodyBuffer{IOBuf::CREATE, header.bodyLength};
  recvFull(fd, bodyBuffer.writableData(), header.bodyLength);
  bodyBuffer.append(header.bodyLength);
  return TakeoverData::deserializeBody(header, &bodyBuffer);
}

std::vector<folly::File> recvMountFDs(int fd) {
  std::vector<folly::File> mountFDs;

  while (true) {
    // Set up a cmsghdr to receive the file descriptors
    ControlMsgBuffer cmsgBuffer(
        ControlMsg::kMaxFDs * sizeof(int), SOL_SOCKET, SCM_RIGHTS);

    char dataByte;
    struct iovec iov;
    iov.iov_base = &dataByte;
    iov.iov_len = sizeof(dataByte);

    struct msghdr msg;
    msg.msg_name = nullptr;
    msg.msg_namelen = 0;
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    cmsgBuffer.addToMsg(&msg);
    msg.msg_flags = 0;

    auto bytesReceived = recvmsg(fd, &msg, MSG_CMSG_CLOEXEC);
    folly::checkUnixError(
        bytesReceived, "recvmsg() failed on while receiving mount FDs");
    if (bytesReceived == 0) {
      return mountFDs;
    }

    if (CMSG_FIRSTHDR(&msg)) {
      auto recvCmsg =
          ControlMsg::fromMsg(msg, SOL_SOCKET, SCM_RIGHTS, sizeof(int));
      const int* fds = recvCmsg.getData<int>();
      size_t numFDs = recvCmsg.getDataLength() / sizeof(int);
      DCHECK_EQ(recvCmsg.getDataLength(), numFDs * sizeof(int))
          << "message length must not have partial file descriptors";
      for (size_t n = 0; n < numFDs; ++n) {
        mountFDs.push_back(folly::File(fds[n], true));
      }
    }

    // Check for the MSG_CTRUNC flag.  This will be set if we have hit the
    // maximum number of file descriptors that can be open for this process,
    // and we aren't allowed to receive more FDs.  (This flag will also be set
    // if msg.msg_controllen was too short to receive the control data.
    // However, this should never occur for us in practice, since we always
    // allocate the same size buffer that the sending code uses.)
    //
    // We check this after creating folly::File objects for the fds that we did
    // receive, so that we will close these FDs properly rather than leaking
    // them.
    if (msg.msg_flags & MSG_CTRUNC) {
      throw std::runtime_error(
          "maximum open FD limit hit when trying to receive mount "
          "file descriptors");
    }
    // Make sure we did receive control message data.
    // We check this after MSG_CTRUNC so that we can provide a more helpful
    // error message if MSG_CTRUNC was set.
    //
    // If MSG_CTRUNC wasn't set this means we screwed up the FD transfer
    // procotol somehow and the sender sent normal data without FDs.  This
    // shouldn't happen unless there is a bug or we are talking to a different
    // version of eden that is sending data in a format we don't understand.
    if (!CMSG_FIRSTHDR(&msg)) {
      throw std::runtime_error(
          "no cmsg received when expecting takeover mount file descriptors");
    }
  }
}
} // namespace

TakeoverData takeoverMounts(AbsolutePathPiece socketPath) {
  SocketAddress address;
  address.setFromPath(socketPath.stringPiece());

  // Create the socket
  namespace fsp = folly::portability::sockets;
  int fd = fsp::socket(address.getFamily(), SOCK_STREAM, 0);
  if (fd < 0) {
    folly::throwSystemError("failed to create takeover client socket");
  }
  SCOPE_EXIT {
    close(fd);
  };
  int rv = fcntl(fd, F_SETFD, FD_CLOEXEC);
  if (rv != 0) {
    folly::throwSystemError(
        "failed to set FD_CLOEXEC on takeover client socket");
  }

  // Connect
  sockaddr_storage addrStorage;
  auto addrLen = address.getAddress(&addrStorage);
  rv = fsp::connect(fd, reinterpret_cast<sockaddr*>(&addrStorage), addrLen);
  if (rv < 0) {
    folly::throwSystemError(
        "unable to connect to takeover client socket at ", socketPath);
  }

  // Send our credentials
  sendCredentials(fd);

  // Receive the mount paths and file descriptors
  auto data = recvTakeoverData(fd);
  auto fds = recvMountFDs(fd);
  // Add 2 here for the lock file and the thrift socket
  if (data.mountPoints.size() + 2 != fds.size()) {
    throw std::runtime_error(folly::to<string>(
        "received ",
        data.mountPoints.size(),
        " mount paths, but ",
        fds.size(),
        " FDs (including the lock file FD)"));
  }
  for (size_t n = 0; n < data.mountPoints.size(); ++n) {
    auto& mountInfo = data.mountPoints[n];
    mountInfo.fuseFD = std::move(fds[n]);
  }
  // The final two FDs are for the lock file and the thrift socket
  data.thriftSocket = std::move(fds.back());
  fds.pop_back();
  data.lockFile = std::move(fds.back());

  return data;
}
} // namespace eden
} // namespace facebook
