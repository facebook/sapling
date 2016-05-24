/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "PrivHelperConn.h"

#include <fcntl.h>
#include <folly/Demangle.h>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/ScopeGuard.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <gflags/gflags.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

using folly::io::Appender;
using folly::io::Cursor;
using folly::ByteRange;
using folly::checkUnixError;
using folly::IOBuf;
using folly::StringPiece;
using folly::throwSystemError;
using std::string;

DEFINE_int32(
    privhelperTimeoutSeconds,
    5,
    "How long to wait for the privhelper process to respond to "
    "requests.");

namespace facebook {
namespace eden {
namespace fusell {

PrivHelperConn::PrivHelperConn() {}

PrivHelperConn::PrivHelperConn(int sock) : socket_(sock) {}

PrivHelperConn::PrivHelperConn(PrivHelperConn&& conn) noexcept
    : socket_(conn.socket_) {
  conn.socket_ = -1;
}

PrivHelperConn& PrivHelperConn::operator=(PrivHelperConn&& conn) noexcept {
  if (socket_ != -1) {
    folly::closeNoInt(socket_);
  }
  socket_ = conn.socket_;
  conn.socket_ = -1;
  return *this;
}

PrivHelperConn::~PrivHelperConn() {
  if (socket_ != -1) {
    ::close(socket_);
  }
}

void PrivHelperConn::createConnPair(
    PrivHelperConn& client,
    PrivHelperConn& server) {
  int sockpair[2];
  int rc = socketpair(AF_UNIX, SOCK_STREAM, 0, sockpair);
  checkUnixError(rc, "failed to create socket pair for privhelper");
  SCOPE_FAIL {
    folly::closeNoInt(sockpair[0]);
    folly::closeNoInt(sockpair[1]);
  };

  auto setupSock = [](int sock) {
    int rc = fcntl(sock, F_SETFD, FD_CLOEXEC);
    checkUnixError(rc, "failed to set privhelper socket as close-on-exec");

    // Make sure the socket buffer is big enough to support our maximum message
    // size.
    //
    // We effectively want each message to be treated as an atomic datagram.
    // However, we have to create the socket as SOCK_STREAM rather than
    // SOCK_DGRAM in order to be able to tell when the remote endpoint
    // closes the connection.
    int bufSize = MAX_MSG_LENGTH * 2;
    rc = setsockopt(sock, SOL_SOCKET, SO_SNDBUF, &bufSize, sizeof(bufSize));
    checkUnixError(rc, "failed to set privhelper socket send buffer size");
  };

  setupSock(sockpair[0]);
  setupSock(sockpair[1]);

  // Set a receive timeout on the client process's socket.
  // We don't want to wait forever on the mount helper to perform operations.
  struct timeval tv;
  tv.tv_sec = FLAGS_privhelperTimeoutSeconds;
  tv.tv_usec = 0;
  rc = setsockopt(sockpair[0], SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
  checkUnixError(rc, "failed to set receive timeout on mount helper socket");

  client = PrivHelperConn{sockpair[0]};
  server = PrivHelperConn{sockpair[1]};
}

void PrivHelperConn::close() {
  if (socket_ == -1) {
    LOG(WARNING) << "privhelper connection already closed";
    return;
  }

  folly::closeNoInt(socket_);
  socket_ = -1;
}

void PrivHelperConn::sendMsg(const Message* msg, int fd) {
  CHECK_LE(msg->dataSize, MAX_MSG_LENGTH);

  // Prepare the message iovec
  auto msgSize = msg->getFullLength();
  std::array<struct iovec, 1> vec;
  vec[0].iov_base = const_cast<Message*>(msg);
  vec[0].iov_len = msgSize;

  // Prepare the msghdr
  struct msghdr mh = {
      .msg_name = nullptr,
      .msg_namelen = 0,
      .msg_iov = vec.data(),
      .msg_iovlen = vec.size(),
      .msg_control = nullptr,
      .msg_controllen = 0,
      .msg_flags = 0,
  };

  // Now prepare msg_control, if we have an fd to send.
  //
  // SCM_RIGHTS allows us to send an array of file descriptors if we wanted to,
  // but we currently only ever need to send one.
  constexpr auto cmsgPayloadSize = sizeof(int);
  std::array<char, CMSG_SPACE(cmsgPayloadSize)> ctrlBuf;

  if (fd >= 0) {
    mh.msg_control = ctrlBuf.data();
    mh.msg_controllen = ctrlBuf.size();

    auto cmsg = CMSG_FIRSTHDR(&mh);
    cmsg->cmsg_level = SOL_SOCKET;
    cmsg->cmsg_type = SCM_RIGHTS;
    cmsg->cmsg_len = CMSG_LEN(cmsgPayloadSize);
    memcpy(CMSG_DATA(cmsg), &fd, sizeof(fd));

    mh.msg_controllen = cmsg->cmsg_len;
  }

  // Finally send the message
  while (true) {
    auto bytesSent = sendmsg(socket_, &mh, MSG_NOSIGNAL);
    if (bytesSent >= 0) {
      // Assert that we sent a full message.
      //
      // The only time this should fail is if the buffer is full and we time
      // out.  However, we don't set SO_SNDTIMEO (except in the unit tests), so
      // this should always be true in production.
      CHECK_EQ(bytesSent, msgSize)
          << "privhelper only sent partial message: " << bytesSent
          << " != " << msgSize;
      break;
    } else if (errno == EINTR) {
      continue;
    } else {
      throwSystemError("error sending privhelper message");
    }
  }
}

void PrivHelperConn::recvMsg(Message* msg, folly::File* f) {
  std::array<struct iovec, 1> vec;
  vec[0].iov_base = msg;
  vec[0].iov_len = sizeof(*msg);

  constexpr auto cmsgPayloadSize = sizeof(int);
  std::array<char, CMSG_SPACE(cmsgPayloadSize)> ctrlBuf;
  struct msghdr mh = {
      .msg_name = nullptr,
      .msg_namelen = 0,
      .msg_iov = vec.data(),
      .msg_iovlen = vec.size(),
      .msg_control = ctrlBuf.data(),
      .msg_controllen = ctrlBuf.size(),
      .msg_flags = 0,
  };

  ssize_t bytesRead;
  while (true) {
    bytesRead = recvmsg(socket_, &mh, MSG_CMSG_CLOEXEC);
    if (bytesRead < 0) {
      if (errno == EINTR) {
        continue;
      }
      throwSystemError("error reading from privhelper socket");
    }
    break;
  }
  if (bytesRead == 0) {
    // EOF
    throw PrivHelperClosedError();
  }
  // Make sure we got at least a full header before we try
  // accessing the header data
  if (bytesRead < offsetof(Message, data)) {
    throw std::runtime_error(folly::to<string>(
        "received partial message header from privhelper socket: size=",
        bytesRead));
  }
  // Make sure the control data wasn't truncated
  if (mh.msg_flags & MSG_CTRUNC) {
    throw std::runtime_error(
        "received truncated control message data from "
        "privhelper socket");
  }
  // Make sure we got the full message
  if (bytesRead != msg->getFullLength()) {
    throw std::runtime_error(folly::to<string>(
        "privhelper message size mismatch: received ",
        bytesRead,
        " bytes, expected ",
        msg->getFullLength()));
  }

  // Pull any file descriptor(s) out of the control message data
  folly::File recvdFile;
  for (auto cmsg = CMSG_FIRSTHDR(&mh); cmsg != nullptr;
       cmsg = CMSG_NXTHDR(&mh, cmsg)) {
    if (cmsg->cmsg_level != SOL_SOCKET || cmsg->cmsg_type != SCM_RIGHTS) {
      continue;
    }
    if (cmsg->cmsg_len < CMSG_LEN(sizeof(int))) {
      LOG(ERROR) << "privhelper control data is too short for a "
                    "file descriptor";
      continue;
    }
    // Technically the buffer could contain a full array of FDs here,
    // but our code only ever sends a single one at a time, so we don't
    // bother to check for an array of more than one.
    int fd;
    memcpy(&fd, CMSG_DATA(cmsg), sizeof(fd));
    recvdFile = folly::File(fd, true);

    // We could potentially break here, but continue around the loop just in
    // case there are more SCM_RIGHTS buffers.  We don't expect there to
    // ever be more than one, but it is nice to double check.
  }

  if (f != nullptr) {
    *f = std::move(recvdFile);
  }
}

void PrivHelperConn::serializeMountRequest(
    Message* msg,
    StringPiece mountPoint) {
  msg->msgType = REQ_MOUNT_FUSE;
  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, sizeof(msg->data)};
  buf.clear(); // Mark all the buffer space as unused
  Appender a{&buf, 0};

  a.writeBE<uint32_t>(mountPoint.size());
  a.push(ByteRange(mountPoint));

  msg->dataSize = buf.length();
}

void PrivHelperConn::parseMountRequest(Message* msg, string& mountPoint) {
  CHECK_EQ(msg->msgType, REQ_MOUNT_FUSE);
  CHECK_LE(msg->dataSize, sizeof(msg->data));

  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, msg->dataSize};
  Cursor c{&buf};

  auto size = c.readBE<uint32_t>();
  mountPoint = c.readFixedString(size);
}

void PrivHelperConn::serializeEmptyResponse(Message* msg) {
  msg->msgType = RESP_EMPTY;
  msg->dataSize = 0;
}

void PrivHelperConn::parseEmptyResponse(const Message* msg) {
  if (msg->msgType == RESP_ERROR) {
    rethrowErrorResponse(msg);
  } else if (msg->msgType != RESP_EMPTY) {
    throw std::runtime_error(
        folly::to<string>("unexpected response type: ", msg->msgType));
  }
}

void PrivHelperConn::serializeBindMountRequest(
    Message* msg,
    folly::StringPiece clientPath,
    folly::StringPiece mountPath) {
  msg->msgType = REQ_MOUNT_BIND;
  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, sizeof(msg->data)};
  buf.clear(); // Mark all the buffer space as unused
  Appender a{&buf, 0};

  a.writeBE<uint32_t>(clientPath.size());
  a.push(ByteRange(clientPath));

  a.writeBE<uint32_t>(mountPath.size());
  a.push(ByteRange(mountPath));

  msg->dataSize = buf.length();
}

void PrivHelperConn::parseBindMountRequest(
    Message* msg,
    std::string& clientPath,
    std::string& mountPath) {
  CHECK_EQ(msg->msgType, REQ_MOUNT_BIND);
  CHECK_LE(msg->dataSize, sizeof(msg->data));

  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, msg->dataSize};
  Cursor c{&buf};

  auto clientPathSize = c.readBE<uint32_t>();
  clientPath = c.readFixedString(clientPathSize);
  auto mountPathSize = c.readBE<uint32_t>();
  mountPath = c.readFixedString(mountPathSize);
}

void PrivHelperConn::serializeErrorResponse(
    Message* msg,
    const std::exception& ex) {
  int errnum = 0;
  auto* sysEx = dynamic_cast<const std::system_error*>(&ex);
  if (sysEx != nullptr && sysEx->code().category() == std::system_category()) {
    errnum = sysEx->code().value();
  }

  auto exceptionType = folly::demangle(typeid(ex));
  serializeErrorResponse(msg, ex.what(), errnum, exceptionType);
}

void PrivHelperConn::serializeErrorResponse(
    Message* msg,
    folly::StringPiece message,
    int errnum,
    folly::StringPiece excType) {
  msg->msgType = RESP_ERROR;
  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, sizeof(msg->data)};
  buf.clear(); // Mark all the buffer space as unused
  Appender a{&buf, 0};

  a.writeBE<uint32_t>(errnum);
  a.writeBE<uint32_t>(message.size());
  a.push(ByteRange(message));
  a.writeBE<uint32_t>(excType.size());
  if (!excType.empty()) {
    a.push(excType);
  }

  msg->dataSize = buf.length();
}

void PrivHelperConn::rethrowErrorResponse(const Message* msg) {
  if (msg->msgType != RESP_ERROR) {
    throw std::runtime_error(folly::to<string>(
        "expected error response, but "
        "got type ",
        msg->msgType));
  }
  CHECK_LE(msg->dataSize, sizeof(msg->data));

  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, msg->dataSize};
  Cursor c{&buf};

  int errnum = c.readBE<uint32_t>();
  auto size = c.readBE<uint32_t>();
  auto errmsg = c.readFixedString(size);
  size = c.readBE<uint32_t>();
  auto errtype = c.readFixedString(size);

  if (errnum != 0) {
    // If we have an errnum, rethrow the error as a std::system_error
    //
    // Unfortunately this will generally duplicate the errno message
    // in the exception string.  (errmsg already includes it from when the
    // system_error was first thrown in the privhelper process, and the
    // system_error constructor ends up including it again here.)
    //
    // There doesn't seem to be an easy way to avoid this at the moment,
    // so for now we just live with it.  (We could explicitly search for the
    // error string at the end of errmsg and strip it off if found, but this
    // seems more complicated than it's worth at the moment.)
    throw std::system_error(errnum, std::system_category(), errmsg);
  }
  throw PrivHelperError(errtype, errmsg);
}

PrivHelperError::PrivHelperError(StringPiece remoteExType, StringPiece msg)
    : message_(folly::to<string>(remoteExType, ": ", msg)) {}
}
}
} // facebook::eden::fusell
