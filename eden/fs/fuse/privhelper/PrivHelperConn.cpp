/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"

#include <fcntl.h>
#include <folly/Demangle.h>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/ScopeGuard.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

#include "eden/fs/utils/ControlMsg.h"
#include "eden/fs/utils/SystemError.h"

using folly::ByteRange;
using folly::checkUnixError;
using folly::IOBuf;
using folly::StringPiece;
using folly::throwSystemError;
using folly::io::Appender;
using folly::io::Cursor;
using std::string;

DEFINE_int32(
    privhelperTimeoutSeconds,
    5,
    "How long to wait for the privhelper process to respond to "
    "requests.");

namespace facebook {
namespace eden {

namespace {

template <typename Func>
void serializeMessage(
    PrivHelperConn::Message* msg,
    PrivHelperConn::MsgType type,
    const Func& serializeBody) {
  msg->msgType = type;
  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, sizeof(msg->data)};
  buf.clear();
  Appender a{&buf, 0};

  serializeBody(a);

  msg->dataSize = buf.length();
}

template <typename Func>
void deserializeMessage(
    PrivHelperConn::Message* msg,
    const Func& deserializeBody) {
  CHECK_LE(msg->dataSize, sizeof(msg->data));
  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, msg->dataSize};
  Cursor cursor{&buf};

  deserializeBody(cursor);
}

void serializeString(Appender& a, StringPiece str) {
  a.writeBE<uint32_t>(str.size());
  a.push(ByteRange(str));
}

std::string deserializeString(Cursor& cursor) {
  const auto length = cursor.readBE<uint32_t>();
  return cursor.readFixedString(length);
}

} // unnamed namespace

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
  std::array<int, 2> sockpair;
  checkUnixError(
      socketpair(AF_UNIX, SOCK_STREAM, 0, sockpair.data()),
      "failed to create socket pair for privhelper");
  SCOPE_FAIL {
    folly::closeNoInt(sockpair[0]);
    folly::closeNoInt(sockpair[1]);
  };

  auto setupSock = [](int sock) {
    checkUnixError(
        fcntl(sock, F_SETFD, FD_CLOEXEC),
        "failed to set privhelper socket as close-on-exec");

    // Make sure the socket buffer is big enough to support our maximum message
    // size.
    //
    // We effectively want each message to be treated as an atomic datagram.
    // However, we have to create the socket as SOCK_STREAM rather than
    // SOCK_DGRAM in order to be able to tell when the remote endpoint
    // closes the connection.
    const int bufSize = MAX_MSG_LENGTH * 2;
    checkUnixError(
        setsockopt(sock, SOL_SOCKET, SO_SNDBUF, &bufSize, sizeof(bufSize)),
        "failed to set privhelper socket send buffer size");
  };

  setupSock(sockpair[0]);
  setupSock(sockpair[1]);

  // Set a receive timeout on the client process's socket.
  // We don't want to wait forever on the mount helper to perform operations.
  struct timeval tv;
  tv.tv_sec = FLAGS_privhelperTimeoutSeconds;
  tv.tv_usec = 0;
  checkUnixError(
      setsockopt(sockpair[0], SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv)),
      "failed to set receive timeout on mount helper socket");

  client = PrivHelperConn{sockpair[0]};
  server = PrivHelperConn{sockpair[1]};
}

void PrivHelperConn::close() {
  if (socket_ == -1) {
    XLOG(WARNING) << "privhelper connection already closed";
    return;
  }

  folly::closeNoInt(socket_);
  socket_ = -1;
}

void PrivHelperConn::sendMsg(const Message* msg, int fd) {
  CHECK_LE(msg->dataSize, MAX_MSG_LENGTH);

  // Prepare the message iovec
  const auto msgSize = msg->getFullLength();
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
  ControlMsgBuffer cmsg(sizeof(int), SOL_SOCKET, SCM_RIGHTS);
  if (fd >= 0) {
    *cmsg.getData<int>() = fd;
    cmsg.addToMsg(&mh);
  }

  // Finally send the message
  while (true) {
    const auto bytesSent = sendmsg(socket_, &mh, MSG_NOSIGNAL);
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

  struct msghdr mh = {
      .msg_name = nullptr,
      .msg_namelen = 0,
      .msg_iov = vec.data(),
      .msg_iovlen = vec.size(),
      .msg_control = nullptr,
      .msg_controllen = 0,
      .msg_flags = 0,
  };
  ControlMsgBuffer cmsgBuffer(sizeof(int), SOL_SOCKET, SCM_RIGHTS);
  cmsgBuffer.addToMsg(&mh);

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
  if (bytesRead < static_cast<ssize_t>(offsetof(Message, data))) {
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
  if (static_cast<size_t>(bytesRead) != msg->getFullLength()) {
    throw std::runtime_error(folly::to<string>(
        "privhelper message size mismatch: received ",
        bytesRead,
        " bytes, expected ",
        msg->getFullLength()));
  }

  // Pull any file descriptor(s) out of the control message data
  folly::File recvdFile;
  if (CMSG_FIRSTHDR(&mh) != nullptr) {
    auto recvCmsg =
        ControlMsg::fromMsg(mh, SOL_SOCKET, SCM_RIGHTS, sizeof(int));

    // The SCM_RIGHTS cmsg structure can contain a full array of FDs,
    // but our code only ever sends one at a time.
    DCHECK_EQ(recvCmsg.getDataLength(), sizeof(int));
    recvdFile = folly::File(*recvCmsg.getData<int>(), true);
  }

  if (f != nullptr) {
    *f = std::move(recvdFile);
  }
}

void PrivHelperConn::serializeMountRequest(
    Message* msg,
    StringPiece mountPoint) {
  const auto serializeBody = [mountPoint](Appender& a) {
    serializeString(a, mountPoint);
  };
  serializeMessage(msg, REQ_MOUNT_FUSE, serializeBody);
}

void PrivHelperConn::parseMountRequest(Message* msg, string& mountPoint) {
  CHECK_EQ(msg->msgType, REQ_MOUNT_FUSE);
  const auto parseBody = [&mountPoint](Cursor& cursor) {
    mountPoint = deserializeString(cursor);
  };
  deserializeMessage(msg, parseBody);
}

void PrivHelperConn::serializeUnmountRequest(
    Message* msg,
    StringPiece mountPoint) {
  const auto serializeBody = [mountPoint](Appender& a) {
    serializeString(a, mountPoint);
  };
  serializeMessage(msg, REQ_UNMOUNT_FUSE, serializeBody);
}

void PrivHelperConn::parseUnmountRequest(Message* msg, string& mountPoint) {
  CHECK_EQ(msg->msgType, REQ_UNMOUNT_FUSE);
  const auto parseBody = [&mountPoint](Cursor& cursor) {
    mountPoint = deserializeString(cursor);
  };
  deserializeMessage(msg, parseBody);
}

void PrivHelperConn::serializeTakeoverShutdownRequest(
    Message* msg,
    StringPiece mountPoint) {
  const auto serializeBody = [mountPoint](Appender& a) {
    serializeString(a, mountPoint);
  };
  serializeMessage(msg, REQ_TAKEOVER_SHUTDOWN, serializeBody);
}

void PrivHelperConn::parseTakeoverShutdownRequest(
    Message* msg,
    string& mountPoint) {
  CHECK_EQ(msg->msgType, REQ_TAKEOVER_SHUTDOWN);
  const auto parseBody = [&mountPoint](Cursor& cursor) {
    mountPoint = deserializeString(cursor);
  };
  deserializeMessage(msg, parseBody);
}

void PrivHelperConn::serializeTakeoverStartupRequest(
    Message* msg,
    folly::StringPiece mountPoint,
    const std::vector<std::string>& bindMounts) {
  const auto serializeBody = [mountPoint, &bindMounts](Appender& a) {
    serializeString(a, mountPoint);
    a.writeBE<uint32_t>(bindMounts.size());
    for (const auto& path : bindMounts) {
      serializeString(a, path);
    }
  };
  serializeMessage(msg, REQ_TAKEOVER_STARTUP, serializeBody);
}

void PrivHelperConn::parseTakeoverStartupRequest(
    Message* msg,
    std::string& mountPoint,
    std::vector<std::string>& bindMounts) {
  CHECK_EQ(msg->msgType, REQ_TAKEOVER_STARTUP);
  const auto parseBody = [&mountPoint, &bindMounts](Cursor& cursor) {
    mountPoint = deserializeString(cursor);
    auto n = cursor.readBE<uint32_t>();
    while (n-- != 0) {
      bindMounts.push_back(deserializeString(cursor));
    }
  };
  deserializeMessage(msg, parseBody);
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
  const auto serializeBody = [clientPath, mountPath](Appender& a) {
    serializeString(a, mountPath);
    serializeString(a, clientPath);
  };
  serializeMessage(msg, REQ_MOUNT_BIND, serializeBody);
}

void PrivHelperConn::parseBindMountRequest(
    Message* msg,
    std::string& clientPath,
    std::string& mountPath) {
  CHECK_EQ(msg->msgType, REQ_MOUNT_BIND);
  const auto parseBody = [&clientPath, &mountPath](Cursor& cursor) {
    mountPath = deserializeString(cursor);
    clientPath = deserializeString(cursor);
  };
  deserializeMessage(msg, parseBody);
}

void PrivHelperConn::serializeErrorResponse(
    Message* msg,
    const std::exception& ex) {
  int errnum = 0;
  auto* sysEx = dynamic_cast<const std::system_error*>(&ex);
  if (sysEx != nullptr && isErrnoError(*sysEx)) {
    errnum = sysEx->code().value();
  }

  const auto exceptionType = folly::demangle(typeid(ex));
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

[[noreturn]] void PrivHelperConn::rethrowErrorResponse(const Message* msg) {
  if (msg->msgType != RESP_ERROR) {
    throw std::runtime_error(folly::to<string>(
        "expected error response, but "
        "got type ",
        msg->msgType));
  }
  CHECK_LE(msg->dataSize, sizeof(msg->data));

  IOBuf buf{IOBuf::WRAP_BUFFER, msg->data, msg->dataSize};
  Cursor c{&buf};

  const int errnum = c.readBE<uint32_t>();
  auto size = c.readBE<uint32_t>();
  const auto errmsg = c.readFixedString(size);
  size = c.readBE<uint32_t>();
  const auto errtype = c.readFixedString(size);

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
    throw std::system_error(errnum, std::generic_category(), errmsg);
  }
  throw PrivHelperError(errtype, errmsg);
}

PrivHelperError::PrivHelperError(StringPiece remoteExType, StringPiece msg)
    : message_(folly::to<string>(remoteExType, ": ", msg)) {}

} // namespace eden
} // namespace facebook
