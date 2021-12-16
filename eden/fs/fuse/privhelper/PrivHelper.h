/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Portability.h>
#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <sys/types.h>
#include <chrono>
#include <memory>

namespace folly {
class EventBase;
class File;
template <typename T>
class Future;
struct Unit;
} // namespace folly

namespace facebook::eden {

#ifndef _WIN32

/**
 * A helper class for performing operations that require elevated privileges.
 *
 * This sends the requests to a separate helper process that has the privileges
 * required to peform these operations.
 */
class PrivHelper {
 public:
  virtual ~PrivHelper() {}

  /**
   * Attach the PrivHelper to an EventBase.
   *
   * This specifies the EventBase that the PrivHelper will use to drive I/O
   * operations.
   *
   * This method must be called before using the PrivHelper, and it must be
   * called from the EventBase thread.
   */
  virtual void attachEventBase(folly::EventBase* eventBase) = 0;

  /**
   * Detach the PrivHelper from its EventBase.
   *
   * This method may only be called from the current EventBase thread.
   *
   * No further I/O can be performed on this PrivHelper until it is re-attached
   * to another EventBase.  Any outstanding requests will not complete until the
   * PrivHelper is attached to another EventBase.
   */
  virtual void detachEventBase() = 0;

  /**
   * Ask the privileged helper process to perform a fuse mount.
   *
   * Returns a folly::File object with the file descriptor containing the fuse
   * connection.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::File> fuseMount(
      folly::StringPiece mountPath,
      bool readOnly) = 0;

  FOLLY_NODISCARD virtual folly::Future<folly::Unit> nfsMount(
      folly::StringPiece mountPath,
      folly::SocketAddress mountdAddr,
      folly::SocketAddress nfsdAddr,
      bool readOnly,
      uint32_t iosize,
      bool useReaddirplus) = 0;

  /**
   * Ask the privileged helper process to perform a fuse unmount.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> fuseUnmount(
      folly::StringPiece mountPath) = 0;

  FOLLY_NODISCARD virtual folly::Future<folly::Unit> nfsUnmount(
      folly::StringPiece mountPath) = 0;

  /**
   * @param clientPath Absolute path to the existing directory where the "real"
   *     storage is.
   * @param mountPath Absolute path to the mount point directory where the bind
   *     mount should be created.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> bindMount(
      folly::StringPiece clientPath,
      folly::StringPiece mountPath) = 0;

  FOLLY_NODISCARD virtual folly::Future<folly::Unit> bindUnMount(
      folly::StringPiece mountPath) = 0;

  /**
   * Inform the privhelper process that we are shutting down as part of a
   * graceful restart, and a new edenfs daemon will take over our existing
   * mount points without unmounting them.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> takeoverShutdown(
      folly::StringPiece mountPath) = 0;

  /**
   * Inform the privhelper process that we have taken over an existing mount
   * point from another edenfs process.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> takeoverStartup(
      folly::StringPiece mountPath,
      const std::vector<std::string>& bindMounts) = 0;

  /**
   * Tell the privhelper server to write all future log messages to the
   * specified file descriptor.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> setLogFile(
      folly::File logFile) = 0;

  /**
   * Tell the privhelper server to use `duration` for the `daemon_timeout`
   * parameter in subsequent fuseMount requests.
   * The `daemon_timeout` is a macOS specific FUSE implementation detail;
   * it is equivalent to our FuseChannel::fuseRequestTimeout_ value, except
   * that the consequence of exceeding the timeout is that the FUSE session
   * is torn down. */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> setDaemonTimeout(
      std::chrono::nanoseconds duration) = 0;

  /**
   * Tell the privhelper server whether it should try loading /dev/edenfs
   * rather than the system fuse implementation.
   */
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> setUseEdenFs(
      bool useEdenFs) = 0;

  /**
   * setLogFileBlocking() is a wrapper around setLogFile() that blocks until
   * the call has completed.
   *
   * This method may only be called if the PrivHelper is not currently attached
   * to an EventBase.  This is primarily intended as a convenience method to
   * allow calling setLogFile() before the main process's EventBase loop has
   * started.
   */
  void setLogFileBlocking(folly::File logFile);
  void setDaemonTimeoutBlocking(std::chrono::nanoseconds duration);
  void setUseEdenFsBlocking(bool useEdenFs);

  /*
   * Explicitly stop the privhelper process.
   *
   * The privhelper process will exit automatically when the main process exits
   * even if this method is not called.  However, this method can be used to
   * explictly stop the privhelper process, and check its exit code.
   *
   * Note that when the privhelper is stopped it will unmount any outstanding
   * mounts points.
   *
   * If the privhelper exited normally, the exit code is returned.
   * If the privhelper was terminated due to a signal, the signal number is
   * returned as a negative number.
   *
   * Throws an exception if the privhelper was not running, or if any other
   * error occurs.
   */
  virtual int stop() = 0;

  /**
   * Returns the underlying file descriptor value.
   * This is intended to be used to pass the privhelper_fd option down
   * to a child process and it must not to used for general reading/writing.
   */
  virtual int getRawClientFd() const = 0;

  /**
   * Checks whether the PrivHelper client can talk to the server by checking
   * if the connection is open and able to take new requests.
   * Returns true if so, false if not.
   */
  virtual bool checkConnection() = 0;
};

#else // _WIN32

/**
 * A stub PrivHelper class for Windows.
 *
 * We do not actually use a separate privhelper process on Windows.  However,
 * for ease of sharing server initialization code across platforms we still
 * define a PrivHelper object, but it does nothing.
 */
class PrivHelper {
 public:
  void setLogFileBlocking(folly::File logFile);
  void setDaemonTimeoutBlocking(std::chrono::nanoseconds duration);
  void setUseEdenFsBlocking(bool useEdenFs);
};

#endif // _WIN32

} // namespace facebook::eden
