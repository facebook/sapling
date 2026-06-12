/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <eden/common/utils/SpawnedProcess.h>
#ifndef __APPLE__
#include <folly/File.h>
#endif
#include <sys/types.h>
#include <limits>
#include <map>
#include <string>
#include <unordered_map>
#include "eden/common/utils/UnixSocket.h"
#include "eden/fs/privhelper/PrivHelperConn.h"

namespace folly {
class EventBase;
class File;
class SocketAddress;
namespace io {
class Cursor;
}
} // namespace folly

namespace facebook::eden {

struct FileAccessMonitorProcess {
  SpawnedProcess proc;

  std::string tmpOutputPath;
  std::string specifiedOutputPath;
  bool shouldUpload;

  FileAccessMonitorProcess(
      SpawnedProcess p,
      std::string tmpOutputPath,
      std::string specifiedOutputPath,
      bool shouldUpload)
      : proc(std::move(p)),
        tmpOutputPath(std::move(tmpOutputPath)),
        specifiedOutputPath(std::move(specifiedOutputPath)),
        shouldUpload(shouldUpload) {}
};

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
  virtual void init(folly::File socket, uid_t uid, gid_t gid);

  /**
   * Initialize the PrivHelperServer without calling folly::init().
   *
   * This can be used if folly::init() has already been called.
   */
  void initPartial(folly::File socket, uid_t uid, gid_t gid);

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
      PrivHelperConn::PrivHelperPacket& packet,
      folly::io::Cursor& cursor,
      UnixSocket::Message& request);
  UnixSocket::Message makeResponse();
  UnixSocket::Message makeResponse(folly::File file);

  UnixSocket::Message processMountMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processMountNfsMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processUnmountMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processNfsUnmountMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processBindMountMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processBindUnMountMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processTakeoverShutdownMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processTakeoverStartupMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processSetLogFileMsg(
      folly::io::Cursor& cursor,
      UnixSocket::Message& request);
  std::string findMatchingMountPrefix(folly::StringPiece path);
  struct RegisteredMount {
#ifndef __APPLE__
    folly::File rootFd;
#endif
  };
#ifndef __APPLE__
  struct FuseMountResult {
    folly::File fuseDev;
    RegisteredMount registeredMount;
  };
  struct CheckedMountPoint {
    folly::File targetFd;
    SanityCheckResult sanityResult;
  };
#endif
  RegisteredMount openRegisteredMount(const std::string& mountPath);
  void registerMountPoint(const std::string& mountPath);
  void registerMountPoint(
      const std::string& mountPath,
      RegisteredMount registeredMount);
  void unmountRegisteredMount(
      const std::string& mountPath,
      const RegisteredMount& registeredMount,
      UnmountOptions options);

  UnixSocket::Message processSetDaemonTimeout(
      folly::io::Cursor& cursor,
      UnixSocket::Message& request);
  UnixSocket::Message processSetUseEdenFs(
      folly::io::Cursor& cursor,
      UnixSocket::Message& request);
  UnixSocket::Message processGetPid();
  UnixSocket::Message processGetNamespaceInfo(folly::io::Cursor& cursor);
  UnixSocket::Message processStartFam(folly::io::Cursor& cursor);
  UnixSocket::Message processStopFam();
  UnixSocket::Message processSetMemoryPriorityForProcess(
      folly::io::Cursor& cursor);
  UnixSocket::Message processSetFuseReadAhead(folly::io::Cursor& cursor);

  void unmountStaleMount(const std::string& mountPoint);

  // Clean up stale redirection mounts under a checkout path that were left
  // behind when EdenFS crashed without properly unmounting.
  SanityCheckResult cleanupStaleBindMounts(const std::string& checkoutPath);

  // Uses stat to determine if there's a stale mount point at the given path. If
  // there is, force unmounts it. Returns true if a stale mount was found and
  // unmounted.
  bool detectAndUnmountStaleMount(
      const std::string& mountPoint,
      bool isNFS,
      bool isHardMount);

  /**
   * Verify that the user has the right credentials to mount/unmount this path.
   *
   * This will check that the user has RW access to every path component
   * leading to the mount point. A std::domain_error exception will be raised
   * if the user doesn't have access to the mount point.
   *
   * When performBindMountCleanup is true (the default), stale redirection
   * bind mounts under the checkout are detached after the checkout path passes
   * the ownership and access checks. The takeover path passes false because
   * the kernel preserves legitimate bind mounts (e.g. Sapling redirections like
   * buck-out) across a graceful restart, and running cleanup there would
   * unmount live user state.
   */
  SanityCheckResult sanityCheckMountPoint(
      const std::string& mountPoint,
      bool isNFS = false,
      bool isHardMount = false,
      bool performBindMountCleanup = true);
#ifndef __APPLE__
  CheckedMountPoint openAndSanityCheckMountPoint(
      const std::string& mountPoint,
      bool isNFS = false,
      bool isHardMount = false,
      bool performBindMountCleanup = true);
#endif

  // These methods are virtual so we can override them during unit tests
  virtual folly::File
  fuseMount(const char* mountPath, bool readOnly, const char* vfsType);
  virtual void nfsMount(std::string mountPath, NFSMountOptions options);
  virtual void unmount(const char* mountPath, UnmountOptions options);
  // Both clientPath and mountPath must be existing directories.
  virtual void insecureBindMount(const char* clientPath, const char* mountPath);
  virtual void bindMount(
      const char* clientPath,
      const char* mountPath,
      folly::StringPiece mountRoot);
  virtual bool useModernMountApi() const;
#ifndef __APPLE__
  FuseMountResult fuseMountByFd(
      folly::File targetFd,
      const char* mountPath,
      bool readOnly,
      const char* vfsType);
  RegisteredMount nfsMountByFd(
      folly::File targetFd,
      const std::string& mountPath,
      const NFSMountOptions& options);
#endif

 protected:
  folly::File openBindMountTarget(
      folly::StringPiece mountRoot,
      folly::StringPiece mountPath);

 private:
  virtual void insecureBindUnmount(const char* mountPath);
  virtual void bindUnmount(const char* mountPath, folly::StringPiece mountRoot);
  virtual void setLogFile(folly::File logFile);
  virtual void setDaemonTimeout(std::chrono::nanoseconds duration);
  virtual void setMemoryPriorityForProcess(pid_t pid, int priority);

  std::unique_ptr<folly::EventBase> eventBase_;
  UnixSocket::UniquePtr conn_;
  uid_t uid_{std::numeric_limits<uid_t>::max()};
  gid_t gid_{std::numeric_limits<gid_t>::max()};
  std::chrono::nanoseconds fuseTimeout_{std::chrono::seconds(60)};
  bool useDevEdenFs_{false};
  std::unique_ptr<FileAccessMonitorProcess> famProcess_;

  // The privhelper server only has a single thread,
  // so we don't need to lock the following state
  std::map<std::string, RegisteredMount> mountPoints_;
};

} // namespace facebook::eden
