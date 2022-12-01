/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/io/Cursor.h>
#include <cinttypes>
#include <stdexcept>
#include "eden/fs/utils/UnixSocket.h"

namespace folly {
class File;
namespace io {
class Appender;
class Cursor;
} // namespace io
} // namespace folly

namespace facebook::eden {

/**
 * This class contains static methods for serializing and deserializing
 * privhelper messages.
 *
 * We use our own simple code for this (rather than thrift, for example)
 * since we need to also pass file descriptors around using SCM_RIGHTS.
 * We also only want to talk over our local socketpair--only the main eden
 * process should be able to make requests to the privileged helper.
 */
class PrivHelperConn {
 public:
  enum MsgType : uint32_t {
    MSG_TYPE_NONE = 0,
    RESP_ERROR = 1,
    REQ_MOUNT_FUSE = 2,
    REQ_MOUNT_BIND = 3,
    REQ_UNMOUNT_FUSE = 4,
    REQ_TAKEOVER_SHUTDOWN = 5,
    REQ_TAKEOVER_STARTUP = 6,
    REQ_SET_LOG_FILE = 7,
    REQ_UNMOUNT_BIND = 8,
    REQ_SET_DAEMON_TIMEOUT = 9,
    REQ_SET_USE_EDENFS = 10,
    REQ_MOUNT_NFS = 11,
    REQ_UNMOUNT_NFS = 12,
  };

  // This structure should never change. If fields need to be added to the
  // header, they should be added to the PrivHelperPacketMetadata struct
  struct PrivHelperPacketHeader {
    uint32_t version;
    // sizeof(PrivHelperPacketMetadata)
    uint32_t length;
  };

  struct PrivHelperPacketMetadata {
    uint32_t transaction_id;
    uint32_t msg_type;
  };

  // Any changes to this structure need to be accompanied by a bump to the
  // version number defined in PrivHelperConn.cpp
  struct PrivHelperPacket {
    PrivHelperPacketHeader header;
    PrivHelperPacketMetadata metadata;
  };

  static PrivHelperPacket parsePacket(folly::io::Cursor& cursor);
  static void serializeResponsePacket(
      PrivHelperPacket& packet,
      folly::io::RWPrivateCursor& cursor);

  /*
   * Create a pair of connected PrivHelperConn objects to use for privhelper
   * communication.
   */
  static void createConnPair(folly::File& client, folly::File& server);

  /*
   * Message serialization and deserialization functions
   *
   * For the parse*() methods the cursor should start at the message body,
   * immediately after the header.  (The caller must have already read the
   * header to determine the message type.)
   */
  static UnixSocket::Message serializeMountRequest(
      uint32_t xid,
      folly::StringPiece mountPoint,
      bool readOnly);
  static void parseMountRequest(
      folly::io::Cursor& cursor,
      std::string& mountPoint,
      bool& readOnly);

  static UnixSocket::Message serializeMountNfsRequest(
      uint32_t xid,
      folly::StringPiece mountPoint,
      folly::SocketAddress mountdAddr,
      folly::SocketAddress nfsdAddr,
      bool readOnly,
      uint32_t iosize,
      bool useReaddirplus);
  static void parseMountNfsRequest(
      folly::io::Cursor& cursor,
      std::string& mountPoint,
      folly::SocketAddress& mountdAddr,
      folly::SocketAddress& nfsdAddr,
      bool& readOnly,
      uint32_t& iosize,
      bool& useReaddirplus);

  static UnixSocket::Message serializeUnmountRequest(
      uint32_t xid,
      folly::StringPiece mountPoint);
  static void parseUnmountRequest(
      folly::io::Cursor& cursor,
      std::string& mountPoint);

  static UnixSocket::Message serializeNfsUnmountRequest(
      uint32_t xid,
      folly::StringPiece mountPoint);
  static void parseNfsUnmountRequest(
      folly::io::Cursor& cursor,
      std::string& mountPoint);

  static UnixSocket::Message serializeBindMountRequest(
      uint32_t xid,
      folly::StringPiece clientPath,
      folly::StringPiece mountPath);
  static void parseBindMountRequest(
      folly::io::Cursor& cursor,
      std::string& clientPath,
      std::string& mountPath);

  static UnixSocket::Message serializeBindUnMountRequest(
      uint32_t xid,
      folly::StringPiece mountPath);
  static void parseBindUnMountRequest(
      folly::io::Cursor& cursor,
      std::string& mountPath);

  static UnixSocket::Message serializeTakeoverShutdownRequest(
      uint32_t xid,
      folly::StringPiece mountPoint);
  static void parseTakeoverShutdownRequest(
      folly::io::Cursor& cursor,
      std::string& mountPoint);

  static UnixSocket::Message serializeTakeoverStartupRequest(
      uint32_t xid,
      folly::StringPiece mountPoint,
      const std::vector<std::string>& bindMounts);
  static void parseTakeoverStartupRequest(
      folly::io::Cursor& cursor,
      std::string& mountPoint,
      std::vector<std::string>& bindMounts);

  static UnixSocket::Message serializeSetLogFileRequest(
      uint32_t xid,
      folly::File logFile);
  static void parseSetLogFileRequest(folly::io::Cursor& cursor);

  static UnixSocket::Message serializeSetDaemonTimeoutRequest(
      uint32_t xid,
      std::chrono::nanoseconds duration);
  static void parseSetDaemonTimeoutRequest(
      folly::io::Cursor& cursor,
      std::chrono::nanoseconds& duration);

  static UnixSocket::Message serializeSetUseEdenFsRequest(
      uint32_t xid,
      bool useEdenFs);
  static void parseSetUseEdenFsRequest(
      folly::io::Cursor& cursor,
      bool& useEdenFs);

  /**
   * Parse a response that is expected to be empty.
   *
   * If the response is an error this will throw an exception from the error
   * data.  Otherwise if the response does not match the expected request type
   * this will also throw an error.
   */
  static void parseEmptyResponse(
      MsgType reqType,
      const UnixSocket::Message& msg);

  static void serializeErrorResponse(
      folly::io::Appender& appender,
      const std::exception& ex);
  static void serializeErrorResponse(
      folly::io::Appender& appender,
      folly::StringPiece message,
      int errnum = 0,
      folly::StringPiece excType = {});
  [[noreturn]] static void rethrowErrorResponse(folly::io::Cursor& cursor);

  static void checkAtEnd(
      const folly::io::Cursor& cursor,
      folly::StringPiece messageType);
};

class PrivHelperError : public std::exception {
 public:
  PrivHelperError(folly::StringPiece remoteExType, folly::StringPiece msg);

  char const* what() const noexcept override {
    return message_.c_str();
  }

 private:
  std::string message_;
};

} // namespace facebook::eden
