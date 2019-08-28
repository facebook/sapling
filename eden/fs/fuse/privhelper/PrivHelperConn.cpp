/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"

#include <fcntl.h>
#include <folly/Demangle.h>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/ScopeGuard.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

#include "eden/fs/utils/SystemError.h"

using folly::ByteRange;
using folly::checkUnixError;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Appender;
using folly::io::Cursor;
using std::string;

namespace facebook {
namespace eden {

namespace {

constexpr size_t kDefaultBufferSize = 1024;

UnixSocket::Message serializeHeader(
    uint32_t xid,
    PrivHelperConn::MsgType type) {
  UnixSocket::Message msg;
  msg.data = IOBuf(IOBuf::CREATE, kDefaultBufferSize);
  Appender appender(&msg.data, kDefaultBufferSize);

  appender.writeBE<uint32_t>(xid);
  appender.writeBE<uint32_t>(static_cast<uint32_t>(type));
  return msg;
}

void serializeString(Appender& a, StringPiece str) {
  a.writeBE<uint32_t>(str.size());
  a.push(ByteRange(str));
}

std::string deserializeString(Cursor& cursor) {
  const auto length = cursor.readBE<uint32_t>();
  return cursor.readFixedString(length);
}

// Helper for setting close-on-exec.  Not needed on systems
// that can atomically do this in socketpair
void setCloExecIfNoSockCloExec(int fd) {
#ifndef SOCK_CLOEXEC
  auto flags = fcntl(fd, F_GETFD);
  folly::checkPosixError(flags);
  folly::checkPosixError(fcntl(fd, F_SETFD, flags | FD_CLOEXEC));
#else
  (void)fd;
#endif
}

} // unnamed namespace

void PrivHelperConn::createConnPair(folly::File& client, folly::File& server) {
  std::array<int, 2> sockpair;
  checkUnixError(
      socketpair(
          AF_UNIX,
          SOCK_STREAM
#ifdef SOCK_CLOEXEC
              | SOCK_CLOEXEC
#endif
          ,
          0,
          sockpair.data()),
      "failed to create socket pair for privhelper");
  setCloExecIfNoSockCloExec(sockpair[0]);
  setCloExecIfNoSockCloExec(sockpair[1]);
  client = folly::File{sockpair[0], /*ownsFd=*/true};
  server = folly::File{sockpair[1], /*ownsFd=*/true};
}

UnixSocket::Message PrivHelperConn::serializeMountRequest(
    uint32_t xid,
    StringPiece mountPoint) {
  auto msg = serializeHeader(xid, REQ_MOUNT_FUSE);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  return msg;
}

void PrivHelperConn::parseMountRequest(Cursor& cursor, string& mountPoint) {
  mountPoint = deserializeString(cursor);
  checkAtEnd(cursor, "mount request");
}

UnixSocket::Message PrivHelperConn::serializeUnmountRequest(
    uint32_t xid,
    StringPiece mountPoint) {
  auto msg = serializeHeader(xid, REQ_UNMOUNT_FUSE);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  return msg;
}

void PrivHelperConn::parseUnmountRequest(Cursor& cursor, string& mountPoint) {
  mountPoint = deserializeString(cursor);
  checkAtEnd(cursor, "unmount request");
}

UnixSocket::Message PrivHelperConn::serializeTakeoverShutdownRequest(
    uint32_t xid,
    StringPiece mountPoint) {
  auto msg = serializeHeader(xid, REQ_TAKEOVER_SHUTDOWN);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  return msg;
}

void PrivHelperConn::parseTakeoverShutdownRequest(
    Cursor& cursor,
    string& mountPoint) {
  mountPoint = deserializeString(cursor);
  checkAtEnd(cursor, "takeover shutdown request");
}

UnixSocket::Message PrivHelperConn::serializeTakeoverStartupRequest(
    uint32_t xid,
    folly::StringPiece mountPoint,
    const std::vector<std::string>& bindMounts) {
  auto msg = serializeHeader(xid, REQ_TAKEOVER_STARTUP);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  appender.writeBE<uint32_t>(bindMounts.size());
  for (const auto& path : bindMounts) {
    serializeString(appender, path);
  }
  return msg;
}

void PrivHelperConn::parseTakeoverStartupRequest(
    Cursor& cursor,
    std::string& mountPoint,
    std::vector<std::string>& bindMounts) {
  mountPoint = deserializeString(cursor);
  auto n = cursor.readBE<uint32_t>();
  while (n-- != 0) {
    bindMounts.push_back(deserializeString(cursor));
  }
  checkAtEnd(cursor, "takeover startup request");
}

void PrivHelperConn::parseEmptyResponse(
    MsgType reqType,
    const UnixSocket::Message& msg) {
  Cursor cursor(&msg.data);
  auto xid = cursor.readBE<uint32_t>();
  auto msgType = static_cast<MsgType>(cursor.readBE<uint32_t>());

  if (msgType == RESP_ERROR) {
    rethrowErrorResponse(cursor);
  } else if (msgType != reqType) {
    throw std::runtime_error(folly::to<string>(
        "unexpected response type ",
        msgType,
        " for request ",
        xid,
        " of type ",
        reqType));
  }
}

UnixSocket::Message PrivHelperConn::serializeBindMountRequest(
    uint32_t xid,
    folly::StringPiece clientPath,
    folly::StringPiece mountPath) {
  auto msg = serializeHeader(xid, REQ_MOUNT_BIND);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPath);
  serializeString(appender, clientPath);
  return msg;
}

void PrivHelperConn::parseBindMountRequest(
    Cursor& cursor,
    std::string& clientPath,
    std::string& mountPath) {
  mountPath = deserializeString(cursor);
  clientPath = deserializeString(cursor);
  checkAtEnd(cursor, "bind mount request");
}

UnixSocket::Message PrivHelperConn::serializeSetDaemonTimeoutRequest(
    uint32_t xid,
    std::chrono::nanoseconds duration) {
  auto msg = serializeHeader(xid, REQ_SET_DAEMON_TIMEOUT);
  Appender appender(&msg.data, kDefaultBufferSize);
  uint64_t durationNanoseconds = duration.count();
  appender.writeBE<uint64_t>(durationNanoseconds);

  return msg;
}

void PrivHelperConn::parseSetDaemonTimeoutRequest(
    Cursor& cursor,
    std::chrono::nanoseconds& duration) {
  duration = std::chrono::nanoseconds(cursor.readBE<uint64_t>());
  checkAtEnd(cursor, "set daemon timeout request");
}

UnixSocket::Message PrivHelperConn::serializeBindUnMountRequest(
    uint32_t xid,
    folly::StringPiece mountPath) {
  auto msg = serializeHeader(xid, REQ_UNMOUNT_BIND);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPath);
  return msg;
}

void PrivHelperConn::parseBindUnMountRequest(
    Cursor& cursor,
    std::string& mountPath) {
  mountPath = deserializeString(cursor);
  checkAtEnd(cursor, "bind mount request");
}

UnixSocket::Message PrivHelperConn::serializeSetLogFileRequest(
    uint32_t xid,
    folly::File logFile) {
  auto msg = serializeHeader(xid, REQ_SET_LOG_FILE);
  msg.files.push_back(std::move(logFile));
  return msg;
}

void PrivHelperConn::parseSetLogFileRequest(folly::io::Cursor& cursor) {
  // REQ_SET_LOG_FILE has an empty body.  The only contents
  // are the file descriptor transferred with the request.
  checkAtEnd(cursor, "set log file request");
}

void PrivHelperConn::serializeErrorResponse(
    Appender& appender,
    const std::exception& ex) {
  int errnum = 0;
  auto* sysEx = dynamic_cast<const std::system_error*>(&ex);
  if (sysEx != nullptr && isErrnoError(*sysEx)) {
    errnum = sysEx->code().value();
  }

  const auto exceptionType = folly::demangle(typeid(ex));
  serializeErrorResponse(appender, ex.what(), errnum, exceptionType);
}

void PrivHelperConn::serializeErrorResponse(
    Appender& appender,
    folly::StringPiece message,
    int errnum,
    folly::StringPiece excType) {
  appender.writeBE<uint32_t>(errnum);
  serializeString(appender, message);
  serializeString(appender, excType);
}

[[noreturn]] void PrivHelperConn::rethrowErrorResponse(Cursor& cursor) {
  const int errnum = cursor.readBE<uint32_t>();
  const auto errmsg = deserializeString(cursor);
  const auto excType = deserializeString(cursor);

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
  throw PrivHelperError(excType, errmsg);
}

void PrivHelperConn::checkAtEnd(const Cursor& cursor, StringPiece messageType) {
  if (!cursor.isAtEnd()) {
    throw std::runtime_error(folly::to<string>(
        "unexpected trailing data at end of ",
        messageType,
        ": ",
        cursor.totalLength(),
        " bytes"));
  }
}

PrivHelperError::PrivHelperError(StringPiece remoteExType, StringPiece msg)
    : message_(folly::to<string>(remoteExType, ": ", msg)) {}

} // namespace eden
} // namespace facebook
