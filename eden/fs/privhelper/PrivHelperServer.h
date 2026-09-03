/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <eden/common/utils/SpawnedProcess.h>
#include <folly/File.h>
#include <sys/types.h>
#include <functional>
#include <limits>
#include <map>
#include <optional>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>
#include "eden/common/utils/PathFuncs.h"
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

/**
 * Move the calling process into its own session (and therefore its own
 * process group), away from the process group it was spawned into.
 *
 * The privhelper must only exit when the EdenFS daemon's connection
 * closes: it unmounts the daemon's mounts on the way out. The daemon
 * detaches itself into a new process group during startup, but the
 * privhelper is spawned earlier and would otherwise remain in the group
 * of whatever launched `eden start`/`eden restart`. Tools that clean up
 * by killing the process group they spawned (e.g. agent command runners)
 * would then SIGKILL the privhelper — which cannot be caught — while the
 * daemon survives, leaving EdenFS unable to mount or unmount.
 *
 * Failure (e.g. the process is already a process-group leader, so there
 * is no foreign group to escape from) is logged and ignored.
 */
void detachFromParentProcessGroup();

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
  UnixSocket::Message processLegacyMacFuseConfigRequest(
      folly::io::Cursor& cursor);
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

  // The daemon's credentials, as supplied to init().
  uid_t uid_{std::numeric_limits<uid_t>::max()};
  gid_t gid_{std::numeric_limits<gid_t>::max()};

  // Virtual so that a test can observe whether a code path skipped it.
  virtual void cleanupMountPoints();

#ifdef __APPLE__
  /**
   * The command to relaunch edenfs with, as read out of the restart sentinel.
   * argv is already stripped of sudo, `--takeover` and inherited file
   * descriptor arguments by the daemon that wrote it.
   */
  struct RelaunchCommand {
    std::vector<std::string> argv;
    std::vector<std::pair<std::string, std::string>> env;
  };

  /**
   * Spawns a new edenfs. Overridable so that tests can exercise the restart
   * decision without launching anything. Returns false if the spawn failed.
   */
  using SpawnEdenFsFn = std::function<bool(
      const AbsolutePath& binary,
      const std::vector<std::string>& argv,
      const std::vector<std::pair<std::string, std::string>>& env)>;
  SpawnEdenFsFn spawnEdenFs_;

  // Seconds since the epoch. Overridable so that tests can age the restart
  // window without sleeping.
  std::function<uint64_t()> now_;

  // Restart configuration most recently supplied by the daemon, if any. Absent
  // means the daemon never finished starting up, so there is nothing to
  // restart and no boot-crash loop is possible.
  std::optional<EdenFsRestartArgs> restartConfig_;

  // Whether the daemon announced that its shutdown was deliberate.
  bool cleanShutdownNotified_{false};

  /** The directory holding the restart sentinel, and its name within it. */
  struct SentinelLocation {
    folly::File dir;
    std::string name;
  };

  // Resolved on first use, and dropped whenever fresh restart args arrive.
  mutable std::optional<SentinelLocation> sentinelLocation_;

  // errno of the last resolution failure reported since the last re-arm, 0 for
  // the malformed-path failure, which has none. A failure is retried rather
  // than cached, so that a transient one cannot leave root permanently unable
  // to restart edenfs; a retry that fails the same way again does not log.
  mutable std::optional<int> lastSentinelResolutionError_;

  /**
   * Where the restart sentinel lives, or nullptr when no restart configuration
   * has arrived or the configured path cannot be resolved. The path comes from
   * the unprivileged daemon, so root walks it once and afterwards only looks
   * the leaf up in the pinned directory, which no ancestor rename can redirect.
   */
  const SentinelLocation* sentinelLocation() const;

  /**
   * The only way to replace the restart configuration. Fresh arguments also
   * re-arm: a resolution cached from the previous configuration would stay
   * pinned to its directory, and a clean-shutdown flag would outlive it.
   */
  void setRestartConfig(EdenFsRestartArgs args);

  UnixSocket::Message processSetRestartArgsMsg(folly::io::Cursor& cursor);
  UnixSocket::Message processNotifyCleanShutdownMsg(folly::io::Cursor& cursor);

  /** Whether edenfs signalled, either way, that it meant to shut down. */
  bool isDisarmed() const;

  /**
   * Parse the relaunch command out of the restart sentinel, or nullopt if it
   * cannot be read or does not hold one. Only ever called with privileges.
   */
  std::optional<RelaunchCommand> readRelaunchCommand() const;

  /**
   * Applies the circuit breaker. Returns false when the limit is reached.
   * Requires restartConfig_ to be set.
   */
  bool admitRestartAttempt();

  /**
   * The edenfs binary installed in `dir`, or nullopt when there is none: it
   * must be a regular, executable file, so a symlinked leaf is rejected.
   */
  static std::optional<AbsolutePath> findSiblingEdenFs(AbsolutePathPiece dir);

  /**
   * Path to the edenfs binary to relaunch: the one installed next to this
   * privhelper, which keeps both on the same version, falling back to argv[0]
   * from the sentinel when there is no sibling. Throws when neither is usable.
   *
   * Virtual because a unit test has no sibling edenfs to point at.
   */
  virtual AbsolutePath resolveEdenFsBinary(
      const RelaunchCommand& command) const;

  /**
   * Verify that resetting the child IDs will select the privhelper's owner.
   * Throws when the real IDs are root or do not match that owner.
   *
   * Virtual so tests can supply controlled credentials.
   */
  virtual void validateRestartOwner() const;
#endif // __APPLE__

 private:
#ifndef __APPLE__
  enum class BindUnmountResult {
    Unmounted,
    AlreadyUnmounted,
    ProcFdUnavailable,
  };

  BindUnmountResult bindUnmountByFd(
      const folly::File& targetFd,
      const char* mountPath);
  virtual int umountBindMountByFd(const char* procFdPath);
#endif
  virtual void insecureBindUnmount(const char* mountPath);
  virtual void bindUnmount(const char* mountPath, folly::StringPiece mountRoot);
  virtual void setLogFile(folly::File logFile);
  virtual void setMemoryPriorityForProcess(pid_t pid, int priority);

  std::unique_ptr<folly::EventBase> eventBase_;
  UnixSocket::UniquePtr conn_;
  std::unique_ptr<FileAccessMonitorProcess> famProcess_;

  // The privhelper server only has a single thread,
  // so we don't need to lock the following state
  std::map<std::string, RegisteredMount> mountPoints_;
};

} // namespace facebook::eden
