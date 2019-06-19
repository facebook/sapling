/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Range.h>
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

namespace facebook {
namespace eden {

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
  };

  /**
   * The length of the message header.
   *
   * This consists of a 32-bit transaction ID followed by
   * the 32-bit request type.
   */
  static constexpr size_t kHeaderSize = 2 * sizeof(uint32_t);

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
      folly::StringPiece mountPoint);
  static void parseMountRequest(
      folly::io::Cursor& cursor,
      std::string& mountPoint);

  static UnixSocket::Message serializeUnmountRequest(
      uint32_t xid,
      folly::StringPiece mountPoint);
  static void parseUnmountRequest(
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

} // namespace eden
} // namespace facebook
