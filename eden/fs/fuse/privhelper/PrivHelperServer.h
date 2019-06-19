/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <sys/types.h>
#include <limits>
#include <set>
#include <string>
#include <unordered_map>
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"
#include "eden/fs/utils/UnixSocket.h"

namespace folly {
class EventBase;
class File;
namespace io {
class Cursor;
}
} // namespace folly

namespace facebook {
namespace eden {

/*
 * PrivHelperServer runs the main loop for the privhelper server process.
 *
 * This processes requests received on the specified socket.
 * The server exits when the remote side of the socket is closed.
 *
 * See PrivHelperConn.h for the various message types.
 *
 * The uid and gid parameters specify the user and group ID of the unprivileged
 * process that will be making requests to us.
 */
class PrivHelperServer : private UnixSocket::ReceiveCallback {
 public:
  PrivHelperServer();
  virtual ~PrivHelperServer();

  /**
   * Initialize the PrivHelperServer.  This should be called prior to run().
   *
   * This calls folly::init().
   */
  virtual void init(folly::File&& socket, uid_t uid, gid_t gid);

  /**
   * Initialize the PrivHelperServer without calling folly::init().
   *
   * This can be used if folly::init() has already been called.
   */
  void initPartial(folly::File&& socket, uid_t uid, gid_t gid);

  /**
   * Run the PrivHelperServer main loop.
   */
  void run();

 private:
  void cleanupMountPoints();

  // UnixSocket::ReceiveCallback methods
  void messageReceived(UnixSocket::Message&& message) noexcept override;
  void eofReceived() noexcept override;
  void socketClosed() noexcept override;
  void receiveError(const folly::exception_wrapper& ew) noexcept override;

  void processAndSendResponse(UnixSocket::Message&& message);
  UnixSocket::Message processMessage(
      PrivHelperConn::MsgType msgType,
      folly::io::Cursor& cursor,
      UnixSocket::Message& request);
  UnixSocket::Message makeResponse();
  UnixSocket::Message makeResponse(folly::File&& file);

  UnixSocket::Message processMountMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processUnmountMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processBindMountMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processTakeoverShutdownMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processTakeoverStartupMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processSetLogFileMsg(
      folly::io::Cursor& cursor,
      UnixSocket::Message& request);

  // These methods are virtual so we can override them during unit tests
  virtual folly::File fuseMount(const char* mountPath);
  virtual void fuseUnmount(const char* mountPath);
  // Both clientPath and mountPath must be existing directories.
  virtual void bindMount(const char* clientPath, const char* mountPath);
  virtual void bindUnmount(const char* mountPath);
  virtual void setLogFile(folly::File&& logFile);

  std::unique_ptr<folly::EventBase> eventBase_;
  UnixSocket::UniquePtr conn_;
  uid_t uid_{std::numeric_limits<uid_t>::max()};
  gid_t gid_{std::numeric_limits<gid_t>::max()};

  // The privhelper server only has a single thread,
  // so we don't need to lock the following state
  std::set<std::string> mountPoints_;
  std::unordered_multimap<std::string, std::string> bindMountPoints_;
};

} // namespace eden
} // namespace facebook
