/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/privhelper/PrivHelperConn.h"

#include <folly/Demangle.h>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/SocketAddress.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Sockets.h>
#include <folly/portability/Unistd.h>

#include "eden/common/utils/Bug.h"
#include "eden/common/utils/SystemError.h"
#include "eden/common/utils/Throw.h"
#include "eden/fs/privhelper/PrivHelper.h"

using folly::ByteRange;
using folly::checkUnixError;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Appender;
using folly::io::Cursor;
using std::string;

namespace facebook::eden {

namespace {

constexpr size_t kDefaultBufferSize = 1024;

// We need to bump this version number any time the protocol is changed. This is
// so that the EdenFS daemon and privhelper daemon understand which version of
// the protocol to use when sending/processing requests and responses.
constexpr uint32_t PRIVHELPER_CURRENT_VERSION = 1;

UnixSocket::Message serializeRequestPacket(
    uint32_t xid,
    PrivHelperConn::MsgType type) {
  XLOGF(
      DBG7,
      "Serializing request packet with v{} protocol. Packet is {} bytes long.",
      PRIVHELPER_CURRENT_VERSION,
      sizeof(PrivHelperConn::PrivHelperPacket));
  UnixSocket::Message msg;
  msg.data = IOBuf(IOBuf::CREATE, kDefaultBufferSize);
  Appender appender(&msg.data, kDefaultBufferSize);

  appender.write<uint32_t>(PRIVHELPER_CURRENT_VERSION);
  appender.write<uint32_t>(sizeof(PrivHelperConn::PrivHelperPacketMetadata));
  appender.write<uint32_t>(xid);
  appender.write<uint32_t>(static_cast<uint32_t>(type));
  return msg;
}

// Template function for serializing an optional value of any type that can be
// trivially serialized (i.e. not strings or socket addresses)
template <
    typename T,
    std::enable_if_t<std::is_arithmetic<T>::value, bool> = true>
void serializeOption(Appender& a, std::optional<T> val) {
  bool is_some = val.has_value();
  a.write<bool>(is_some);
  if (is_some) {
    a.write<T>(val.value());
  }
}

// Template function for deserializing an optional value of any type that can be
//  trivially deserialized (i.e. not strings or socket addresses)
template <
    typename T,
    std::enable_if_t<std::is_arithmetic<T>::value, bool> = true>
std::optional<T> deserializeOption(Cursor& cursor) {
  bool is_some = cursor.read<bool>();
  if (is_some) {
    return cursor.read<T>();
  } else {
    return std::nullopt;
  }
}

void serializeString(Appender& a, StringPiece str) {
  a.write<uint32_t>(str.size());
  a.push(ByteRange(str));
}

std::string deserializeString(Cursor& cursor) {
  const auto length = cursor.read<uint32_t>();
  return cursor.readFixedString(length);
}

void serializeBool(Appender& a, bool b) {
  a.write<uint8_t>(b);
}

bool deserializeBool(Cursor& cursor) {
  return static_cast<bool>(cursor.read<uint8_t>());
}

void serializeUint8(Appender& a, uint8_t val) {
  a.write<uint8_t>(val);
}

uint8_t deserializeUint8(Cursor& cursor) {
  return cursor.read<uint8_t>();
}

void serializeUint16(Appender& a, uint16_t val) {
  a.write<uint16_t>(val);
}

uint16_t deserializeUint16(Cursor& cursor) {
  return cursor.read<uint16_t>();
}

void serializeUint32(Appender& a, uint64_t val) {
  a.write<uint32_t>(val);
}

uint32_t deserializeUint32(Cursor& cursor) {
  return cursor.read<uint32_t>();
}

void serializeInt32(Appender& a, int32_t val) {
  a.write<int32_t>(val);
}

int32_t deserializeInt32(Cursor& cursor) {
  return cursor.read<int32_t>();
}

void serializeSocketAddress(Appender& a, const folly::SocketAddress& addr) {
  bool isInet = addr.isFamilyInet();
  serializeBool(a, isInet);
  if (isInet) {
    serializeString(a, addr.getAddressStr());
    serializeUint16(a, addr.getPort());
  } else {
    XCHECK_EQ(addr.getFamily(), AF_UNIX);
    serializeString(a, addr.getPath());
  }
}

folly::SocketAddress deserializeSocketAddress(Cursor& cursor) {
  bool isInet = deserializeBool(cursor);
  if (isInet) {
    auto host = deserializeString(cursor);
    auto port = deserializeUint16(cursor);
    return folly::SocketAddress(host, port);
  } else {
    auto path = deserializeString(cursor);
    return folly::SocketAddress::makeFromPath(path);
  }
}

void serializeNFSMountOptions(Appender& a, const NFSMountOptions& options) {
  serializeSocketAddress(a, options.mountdAddr);
  serializeSocketAddress(a, options.nfsdAddr);
  serializeBool(a, options.readOnly);
  serializeUint32(a, options.iosize);
  serializeBool(a, options.useReaddirplus);
  serializeBool(a, options.useSoftMount);
  serializeUint32(a, options.readIOSize);
  serializeUint32(a, options.writeIOSize);
  serializeOption(a, options.directoryReadSize);
  serializeUint8(a, options.readAheadSize);
  serializeInt32(a, options.retransmitTimeoutTenthSeconds);
  serializeUint32(a, options.retransmitAttempts);
  serializeInt32(a, options.deadTimeoutSeconds);
  serializeOption(a, options.dumbtimer);
}

NFSMountOptions deserializeNFSMountOptions(Cursor& cursor) {
  NFSMountOptions options;
  options.mountdAddr = deserializeSocketAddress(cursor);
  options.nfsdAddr = deserializeSocketAddress(cursor);
  options.readOnly = deserializeBool(cursor);
  options.iosize = deserializeUint32(cursor);
  options.useReaddirplus = deserializeBool(cursor);
  options.useSoftMount = deserializeBool(cursor);
  options.readIOSize = deserializeUint32(cursor);
  options.writeIOSize = deserializeUint32(cursor);
  options.directoryReadSize = deserializeOption<uint32_t>(cursor);
  options.readAheadSize = deserializeUint8(cursor);
  options.retransmitTimeoutTenthSeconds = deserializeInt32(cursor);
  options.retransmitAttempts = deserializeUint32(cursor);
  options.deadTimeoutSeconds = deserializeInt32(cursor);
  options.dumbtimer = deserializeOption<bool>(cursor);
  return options;
}

void serializeUnmountOptions(Appender& a, const UnmountOptions& options) {
  uint32_t bitset = 0;
  bitset |= options.force ? UnmountOptionBits::FORCE : 0;
  bitset |= options.detach ? UnmountOptionBits::DETACH : 0;
  bitset |= options.expire ? UnmountOptionBits::EXPIRE : 0;
  serializeUint32(a, bitset);
}

void deserializeUnmountOptions(Cursor& cursor, UnmountOptions& options) {
  uint32_t bitset = deserializeUint32(cursor);

  options.force = (bitset & UnmountOptionBits::FORCE) != 0;
  options.detach = (bitset & UnmountOptionBits::DETACH) != 0;
  options.expire = (bitset & UnmountOptionBits::EXPIRE) != 0;
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

PrivHelperConn::PrivHelperPacket PrivHelperConn::parsePacket(Cursor& cursor) {
  // read the size and version from the header
  PrivHelperPacket packet{};
  try {
    packet.header = cursor.read<PrivHelperConn::PrivHelperPacketHeader>();
  } catch (const std::out_of_range& e) {
    throwf<std::runtime_error>(
        "privhelper packet buffer did not include version/length header: {}",
        e.what());
  }

  // read the packet metadata and record how many bytes are read
  size_t pulledBytes = cursor.pullAtMost(
      &packet.metadata,
      std::min<size_t>(
          packet.header.length,
          sizeof(PrivHelperConn::PrivHelperPacketMetadata)));
  XLOGF(
      DBG7,
      "We parsed a v{} packet for a total of {} bytes (header {} + metadata {})",
      packet.header.version,
      sizeof(PrivHelperPacketHeader) + pulledBytes,
      sizeof(PrivHelperPacketHeader),
      pulledBytes);

  // We somehow read more bytes than the header indicated. This
  // should be impossible and indicates a bug
  assert(pulledBytes <= packet.header.length);

  if (pulledBytes < packet.header.length) {
    // We need to advance the cursor since the received packet is larger
    // than we expected it would be
    uint32_t sizeDifference = packet.header.length - pulledBytes;
    XLOGF(
        DBG7,
        "Metadata is larger than expected ({} bytes). Pulled {} bytes, advancing the cursor by {} bytes.",
        packet.header.length,
        pulledBytes,
        sizeDifference);
    cursor.skip(sizeDifference);
  }
  return packet;
}

void PrivHelperConn::serializeResponsePacket(
    PrivHelperConn::PrivHelperPacket& packet,
    folly::io::RWPrivateCursor& cursor) {
  XLOGF(
      DBG7,
      "Serializing response packet with v{} protocol. Packet is {} bytes long.",
      PRIVHELPER_CURRENT_VERSION,
      sizeof(packet));
  cursor.write<uint32_t>(PRIVHELPER_CURRENT_VERSION);
  cursor.write<uint32_t>(sizeof(packet.metadata));
  cursor.write<uint32_t>(packet.metadata.transaction_id);
  cursor.write<uint32_t>(packet.metadata.msg_type);
}

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
    StringPiece mountPoint,
    bool readOnly,
    StringPiece vfsType) {
  auto msg = serializeRequestPacket(xid, REQ_MOUNT_FUSE);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  serializeBool(appender, readOnly);
  serializeString(appender, vfsType);
  return msg;
}

void PrivHelperConn::parseMountRequest(
    Cursor& cursor,
    string& mountPoint,
    bool& readOnly,
    string& vfsType) {
  mountPoint = deserializeString(cursor);
  readOnly = deserializeBool(cursor);
  vfsType = deserializeString(cursor);
  checkAtEnd(cursor, "mount request");
}

UnixSocket::Message PrivHelperConn::serializeMountNfsRequest(
    uint32_t xid,
    folly::StringPiece mountPoint,
    const NFSMountOptions& options) {
  auto msg = serializeRequestPacket(xid, REQ_MOUNT_NFS);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  serializeNFSMountOptions(appender, options);
  return msg;
}

void PrivHelperConn::parseMountNfsRequest(
    folly::io::Cursor& cursor,
    std::string& mountPoint,
    NFSMountOptions& options) {
  mountPoint = deserializeString(cursor);
  options = deserializeNFSMountOptions(cursor);
  checkAtEnd(cursor, "mount nfs request");
}

UnixSocket::Message PrivHelperConn::serializeUnmountRequest(
    uint32_t xid,
    StringPiece mountPoint,
    const UnmountOptions& options) {
  auto msg = serializeRequestPacket(xid, REQ_UNMOUNT_FUSE);

  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  serializeUnmountOptions(appender, options);
  return msg;
}

void PrivHelperConn::parseUnmountRequest(
    Cursor& cursor,
    string& mountPoint,
    UnmountOptions& options) {
  mountPoint = deserializeString(cursor);

  if (!cursor.isAtEnd()) {
    deserializeUnmountOptions(cursor, options);
  }

  checkAtEnd(cursor, "unmount request");
}

UnixSocket::Message PrivHelperConn::serializeNfsUnmountRequest(
    uint32_t xid,
    StringPiece mountPoint) {
  auto msg = serializeRequestPacket(xid, REQ_UNMOUNT_NFS);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  return msg;
}

void PrivHelperConn::parseNfsUnmountRequest(
    Cursor& cursor,
    string& mountPoint) {
  mountPoint = deserializeString(cursor);
  checkAtEnd(cursor, "unmount request");
}

UnixSocket::Message PrivHelperConn::serializeTakeoverShutdownRequest(
    uint32_t xid,
    StringPiece mountPoint) {
  auto msg = serializeRequestPacket(xid, REQ_TAKEOVER_SHUTDOWN);
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
  auto msg = serializeRequestPacket(xid, REQ_TAKEOVER_STARTUP);
  Appender appender(&msg.data, kDefaultBufferSize);

  serializeString(appender, mountPoint);
  appender.write<uint32_t>(bindMounts.size());
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
  auto n = cursor.read<uint32_t>();
  while (n-- != 0) {
    bindMounts.push_back(deserializeString(cursor));
  }
  checkAtEnd(cursor, "takeover startup request");
}

pid_t PrivHelperConn::parseGetPidResponse(const UnixSocket::Message& msg) {
  Cursor cursor(&msg.data);
  PrivHelperPacket packet = parsePacket(cursor);
  if (packet.metadata.msg_type == RESP_ERROR) {
    rethrowErrorResponse(cursor);
  } else if (packet.metadata.msg_type != REQ_GET_PID) {
    throwf<std::runtime_error>(
        "unexpected response type {} for request {} of type {} for version v{}",
        packet.metadata.msg_type,
        packet.metadata.transaction_id,
        REQ_GET_PID,
        packet.header.version);
  }
  pid_t pid;
  bool valid = cursor.tryReadBE<pid_t>(pid);
  if (!valid) {
    throwf<std::runtime_error>(
        "Failed to read pid from privhelper server for request {} for version v{}",
        packet.metadata.transaction_id,
        packet.header.version);
  }
  return pid;
}

void PrivHelperConn::parseEmptyResponse(
    MsgType reqType,
    const UnixSocket::Message& msg) {
  Cursor cursor(&msg.data);
  PrivHelperPacket packet = parsePacket(cursor);

  // In the future, we may parse empty responses differently depending on the
  // the version we get back from the parsed packet. For now, we'll parse all
  // empty responses in the same way.
  if (packet.metadata.msg_type == RESP_ERROR) {
    rethrowErrorResponse(cursor);
  } else if (packet.metadata.msg_type != reqType) {
    throwf<std::runtime_error>(
        "unexpected response type {} for request {} of type {} for version v{}",
        packet.metadata.msg_type,
        packet.metadata.transaction_id,
        reqType,
        packet.header.version);
  }
}

UnixSocket::Message PrivHelperConn::serializeBindMountRequest(
    uint32_t xid,
    folly::StringPiece clientPath,
    folly::StringPiece mountPath) {
  auto msg = serializeRequestPacket(xid, REQ_MOUNT_BIND);
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
  auto msg = serializeRequestPacket(xid, REQ_SET_DAEMON_TIMEOUT);
  Appender appender(&msg.data, kDefaultBufferSize);
  uint64_t durationNanoseconds = duration.count();
  appender.write<uint64_t>(durationNanoseconds);

  return msg;
}

void PrivHelperConn::parseSetDaemonTimeoutRequest(
    Cursor& cursor,
    std::chrono::nanoseconds& duration) {
  duration = std::chrono::nanoseconds(cursor.read<uint64_t>());
  checkAtEnd(cursor, "set daemon timeout request");
}

UnixSocket::Message PrivHelperConn::serializeSetUseEdenFsRequest(
    uint32_t xid,
    bool useEdenFs) {
  auto msg = serializeRequestPacket(xid, REQ_SET_USE_EDENFS);
  Appender appender(&msg.data, kDefaultBufferSize);
  appender.write<uint64_t>(((useEdenFs) ? 1 : 0));

  return msg;
}

void PrivHelperConn::parseSetUseEdenFsRequest(Cursor& cursor, bool& useEdenFs) {
  useEdenFs = bool(cursor.read<uint64_t>());
  checkAtEnd(cursor, "set use /dev/edenfs");
}

UnixSocket::Message PrivHelperConn::serializeGetPidRequest(uint32_t xid) {
  return serializeRequestPacket(xid, REQ_GET_PID);
}

UnixSocket::Message PrivHelperConn::serializeStartFamRequest(
    uint32_t xid,
    const std::vector<std::string>& paths,
    const std::string& tmpOutputPath,
    const std::string& specifiedOutputPath,
    const bool shouldUpload) {
  auto msg = serializeRequestPacket(xid, REQ_START_FAM);
  Appender appender(&msg.data, kDefaultBufferSize);

  appender.write<uint32_t>(paths.size());
  for (const auto& path : paths) {
    serializeString(appender, path);
  }
  serializeString(appender, tmpOutputPath);
  serializeString(appender, specifiedOutputPath);
  serializeBool(appender, shouldUpload);
  return msg;
}

void PrivHelperConn::parseStartFamRequest(
    folly::io::Cursor& cursor,
    std::vector<std::string>& paths,
    std::string& tmpOutputPath,
    std::string& specifiedOutputPath,
    bool& shouldUpload) {
  auto n = cursor.read<uint32_t>();
  while (n-- != 0) {
    paths.push_back(deserializeString(cursor));
  }
  tmpOutputPath = deserializeString(cursor);
  specifiedOutputPath = deserializeString(cursor);
  shouldUpload = deserializeBool(cursor);
  checkAtEnd(cursor, "start fam");
}

void PrivHelperConn::serializeStopFamResponse(
    Appender& appender,
    const std::string& tmpOutputPath,
    const std::string& specifiedOutputPath,
    const bool shouldUpload) {
  serializeString(appender, tmpOutputPath);
  serializeString(appender, specifiedOutputPath);
  serializeBool(appender, shouldUpload);
}

pid_t PrivHelperConn::parseStartFamResponse(const UnixSocket::Message& msg) {
  Cursor cursor(&msg.data);
  PrivHelperPacket packet = parsePacket(cursor);
  if (packet.metadata.msg_type == RESP_ERROR) {
    rethrowErrorResponse(cursor);
  } else if (packet.metadata.msg_type != REQ_START_FAM) {
    throwf<std::runtime_error>(
        "unexpected response type {} for request {} of type {} for version v{}",
        packet.metadata.msg_type,
        packet.metadata.transaction_id,
        REQ_START_FAM,
        packet.header.version);
  }

  pid_t pid;
  bool valid = cursor.tryReadBE<pid_t>(pid);

  if (!valid) {
    throwf<std::runtime_error>(
        "Failed to read pid from privhelper server for request {} for version v{}",
        packet.metadata.transaction_id,
        packet.header.version);
  }

  return pid;
}

void PrivHelperConn::parseStopFamResponse(
    const UnixSocket::Message& msg,
    std::string& tmpOutputPath,
    std::string& specifiedOutputPath,
    bool& shouldUpload) {
  Cursor cursor(&msg.data);
  PrivHelperPacket packet = parsePacket(cursor);
  if (packet.metadata.msg_type == RESP_ERROR) {
    rethrowErrorResponse(cursor);
  } else if (packet.metadata.msg_type != REQ_STOP_FAM) {
    throwf<std::runtime_error>(
        "unexpected response type {} for request {} of type {} for version v{}",
        packet.metadata.msg_type,
        packet.metadata.transaction_id,
        REQ_START_FAM,
        packet.header.version);
  }

  tmpOutputPath = deserializeString(cursor);
  specifiedOutputPath = deserializeString(cursor);
  shouldUpload = deserializeBool(cursor);
}

UnixSocket::Message PrivHelperConn::serializeStopFamRequest(uint32_t xid) {
  return serializeRequestPacket(xid, REQ_STOP_FAM);
}

UnixSocket::Message PrivHelperConn::serializeBindUnMountRequest(
    uint32_t xid,
    folly::StringPiece mountPath) {
  auto msg = serializeRequestPacket(xid, REQ_UNMOUNT_BIND);
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
  auto msg = serializeRequestPacket(xid, REQ_SET_LOG_FILE);
  msg.files.push_back(std::move(logFile));
  return msg;
}

void PrivHelperConn::parseSetLogFileRequest(folly::io::Cursor& cursor) {
  // REQ_SET_LOG_FILE has an empty body.  The only contents
  // are the file descriptor transferred with the request.
  checkAtEnd(cursor, "set log file request");
}

UnixSocket::Message PrivHelperConn::serializeSetMemoryPriorityForProcessRequest(
    uint32_t xid,
    pid_t pid,
    int targetPriority) {
  auto msg = serializeRequestPacket(xid, REQ_SET_MEMORY_PRIORITY_FOR_PROCESS);
  Appender appender(&msg.data, kDefaultBufferSize);
  appender.write<pid_t>(pid);
  appender.write<int>(targetPriority);
  return msg;
}

void PrivHelperConn::parseSetMemoryPriorityForProcessRequest(
    Cursor& cursor,
    pid_t& pid,
    int& targetPriority) {
  pid = cursor.read<pid_t>();
  targetPriority = cursor.read<int>();
  checkAtEnd(cursor, "set memory priority for process request");
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
  appender.write<uint32_t>(errnum);
  serializeString(appender, message);
  serializeString(appender, excType);
}

[[noreturn]] void PrivHelperConn::rethrowErrorResponse(Cursor& cursor) {
  const int errnum = cursor.read<uint32_t>();
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

} // namespace facebook::eden

#endif
