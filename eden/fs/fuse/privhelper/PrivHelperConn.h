/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Range.h>
#include <cinttypes>
#include <stdexcept>
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class File;
}

namespace facebook {
namespace eden {

/*
 * A helper class for sending and receiving messages on the privhelper socket.
 *
 * We use our own simple code for this (rather than thrift, for example)
 * since we need to also pass file descriptors around using SCM_RIGHTS.
 * We also only want to talk over our local socketpair--only the main eden
 * process should be able to make requests to the privileged helper.
 *
 * This class is used by both the client and server side of the socket.
 */
class PrivHelperConn {
 public:
  // The maximum body data size allowed for a privhelper message.
  enum { MAX_MSG_LENGTH = 4000 };

  enum MsgType : uint32_t {
    MSG_TYPE_NONE = 0,
    RESP_ERROR = 1,
    RESP_EMPTY = 2,
    REQ_MOUNT_FUSE = 3,
    REQ_MOUNT_BIND = 4,
    REQ_UNMOUNT_FUSE = 5,
    REQ_TAKEOVER_SHUTDOWN = 6,
    REQ_TAKEOVER_STARTUP = 7,
  };

  struct Message {
    size_t getFullLength() const {
      return offsetof(Message, data) + dataSize;
    }

    uint32_t xid{0}; // transaction ID
    uint32_t msgType{MSG_TYPE_NONE};
    uint32_t dataSize{0}; // Number of bytes populated in data[]
    uint8_t data[MAX_MSG_LENGTH];
  };

  /*
   * Create an uninitialized PrivHelperConn object.
   */
  PrivHelperConn();

  /*
   * Construct a PrivHelperConn from a socket object.
   *
   * Note that you probably just want to use createConnPair()
   * rather than calling this function directly.
   */
  explicit PrivHelperConn(int sock);

  /*
   * Move constructor and move assignment.
   */
  PrivHelperConn(PrivHelperConn&& conn) noexcept;
  PrivHelperConn& operator=(PrivHelperConn&& conn) noexcept;

  ~PrivHelperConn();

  /*
   * Create a pair of connected PrivHelperConn objects to use for privhelper
   * communication.
   */
  static void createConnPair(PrivHelperConn& client, PrivHelperConn& server);

  void close();

  bool isClosed() const {
    return socket_ == -1;
  }

  int getSocket() const {
    return socket_;
  }

  /*
   * Low-level message sending and receiving
   */

  /*
   * Send a message, and optionally a file descriptor.
   *
   * This takes the file descriptor as a raw integer since it does
   * not accept ownership of the fd.  The caller still owns the FD and
   * is responsible for closing it at some later time.
   */
  void sendMsg(const Message* msg, int fd = -1);

  /*
   * Receive a message, and optional a file descriptor.
   *
   * This will populate the data in the Message object passed in by the caller.
   *
   * The file descriptor is returned using a folly::File object,
   * since the receiver is given ownership of the file descriptor,
   * and must close it later.  Use folly::File::release() if you want to
   * extract the raw file descriptor and manage it using some other mechanism.
   *
   * The File argument can be nullptr if you don't expect to receive a file
   * descriptor.
   */
  void recvMsg(Message* msg, folly::File* f);

  /*
   * Message serialization and deserialization functions
   */
  static void serializeMountRequest(
      Message* msg,
      folly::StringPiece mountPoint);
  static void parseMountRequest(Message* msg, std::string& mountPoint);

  static void serializeUnmountRequest(
      Message* msg,
      folly::StringPiece mountPoint);
  static void parseUnmountRequest(Message* msg, std::string& mountPoint);

  static void serializeBindMountRequest(
      Message* msg,
      folly::StringPiece clientPath,
      folly::StringPiece mountPath);
  static void parseBindMountRequest(
      Message* msg,
      std::string& clientPath,
      std::string& mountPath);

  static void serializeTakeoverShutdownRequest(
      Message* msg,
      folly::StringPiece mountPoint);
  static void parseTakeoverShutdownRequest(
      Message* msg,
      std::string& mountPoint);

  static void serializeTakeoverStartupRequest(
      Message* msg,
      folly::StringPiece mountPoint,
      const std::vector<std::string>& bindMounts);
  static void parseTakeoverStartupRequest(
      Message* msg,
      std::string& mountPoint,
      std::vector<std::string>& bindMounts);

  static void serializeEmptyResponse(Message* msg);

  /**
   * Parse a response that is expected to be empty.
   * Will throw an exception if this is actually an error response.
   */
  static void parseEmptyResponse(const Message* msg);

  static void serializeErrorResponse(Message* msg, const std::exception& ex);
  static void serializeErrorResponse(
      Message* msg,
      folly::StringPiece message,
      int errnum = 0,
      folly::StringPiece excType = {});
  [[noreturn]] static void rethrowErrorResponse(const Message* msg);

 private:
  int socket_{-1};
};

class PrivHelperClosedError : public std::exception {
 public:
  char const* what() const noexcept override {
    return "privhelper socket closed";
  }
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
