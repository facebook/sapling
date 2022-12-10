/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/privhelper/PrivHelperImpl.h"

#include <folly/Exception.h>
#include <folly/Expected.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/SocketAddress.h>
#include <folly/String.h>
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/init/Init.h>
#include <folly/io/Cursor.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <folly/portability/SysTypes.h>
#include <folly/portability/Unistd.h>
#ifndef _WIN32
#include <sys/wait.h>
#endif // !_WIN32

#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/fuse/privhelper/PrivHelperFlags.h"

#ifndef _WIN32
#include "eden/fs/fuse/privhelper/PrivHelperConn.h"
#include "eden/fs/fuse/privhelper/PrivHelperServer.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/FileDescriptor.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SpawnedProcess.h"
#include "eden/fs/utils/UserInfo.h"
#endif // _WIN32

using folly::checkUnixError;
using folly::EventBase;
using folly::File;
using folly::Future;
using folly::StringPiece;
using folly::Unit;
using folly::io::Cursor;
using std::make_unique;
using std::string;
using std::unique_ptr;
using std::vector;

DEFINE_string(
    privhelper_path,
    "",
    "The path to the privhelper binary (only works if not running setuid)");

namespace facebook::eden {

#ifndef _WIN32

namespace {

/**
 * PrivHelperClientImpl contains the client-side logic (in the parent process)
 * for talking to the remote privileged process.
 */
class PrivHelperClientImpl : public PrivHelper,
                             private UnixSocket::ReceiveCallback,
                             private UnixSocket::SendCallback,
                             private EventBase::OnDestructionCallback {
 public:
  PrivHelperClientImpl(File&& conn, std::optional<SpawnedProcess> proc)
      : helperProc_(std::move(proc)),
        state_{ThreadSafeData{
            Status::NOT_STARTED,
            nullptr,
            UnixSocket::makeUnique(nullptr, std::move(conn))}} {}
  ~PrivHelperClientImpl() override {
    cleanup();
    XDCHECK_EQ(sendPending_, 0ul);
  }

  void attachEventBase(EventBase* eventBase) override {
    {
      auto state = state_.wlock();
      if (state->status != Status::NOT_STARTED) {
        throwf<std::runtime_error>(
            "PrivHelper::start() called in unexpected state {}",
            static_cast<uint32_t>(state->status));
      }
      state->eventBase = eventBase;
      state->status = Status::RUNNING;
      state->conn_->attachEventBase(eventBase);
      state->conn_->setReceiveCallback(this);
    }
    eventBase->runOnDestruction(*this);
  }

  void detachEventBase() override {
    detachWithinEventBaseDestructor();
    cancel();
  }

  Future<File> fuseMount(folly::StringPiece mountPath, bool readOnly) override;
  Future<Unit> nfsMount(
      folly::StringPiece mountPath,
      folly::SocketAddress mountdAddr,
      folly::SocketAddress nfsdAddr,
      bool readOnly,
      uint32_t iosize,
      bool useReaddirplus) override;
  Future<Unit> fuseUnmount(StringPiece mountPath) override;
  Future<Unit> nfsUnmount(StringPiece mountPath) override;
  Future<Unit> bindMount(StringPiece clientPath, StringPiece mountPath)
      override;
  folly::Future<folly::Unit> bindUnMount(folly::StringPiece mountPath) override;
  Future<Unit> takeoverShutdown(StringPiece mountPath) override;
  Future<Unit> takeoverStartup(
      StringPiece mountPath,
      const vector<string>& bindMounts) override;
  Future<Unit> setLogFile(folly::File logFile) override;
  Future<folly::Unit> setDaemonTimeout(
      std::chrono::nanoseconds duration) override;
  Future<folly::Unit> setUseEdenFs(bool useEdenFs) override;
  int stop() override;
  int getRawClientFd() const override {
    auto state = state_.rlock();
    return state->conn_->getRawFd();
  }
  bool checkConnection() override;

 private:
  using PendingRequestMap =
      std::unordered_map<uint32_t, folly::Promise<UnixSocket::Message>>;
  enum class Status : uint32_t {
    NOT_STARTED,
    RUNNING,
    CLOSED,
    WAITED,
  };
  struct ThreadSafeData {
    Status status;
    EventBase* eventBase;
    UnixSocket::UniquePtr conn_;
  };

  uint32_t getNextXid() {
    return nextXid_.fetch_add(1, std::memory_order_acq_rel);
  }
  /**
   * Close the socket to the privhelper server, and wait for it to exit.
   *
   * Returns the exit status of the privhelper process, or an errno value on
   * error.
   */
  folly::Expected<ProcessStatus, int> cleanup() {
    EventBase* eventBase{nullptr};
    {
      auto state = state_.wlock();
      if (state->status == Status::WAITED) {
        // We have already waited on the privhelper process.
        return folly::makeUnexpected(ESRCH);
      }
      if (state->status == Status::RUNNING) {
        eventBase = state->eventBase;
        state->eventBase = nullptr;
      }
      state->status = Status::WAITED;
    }

    // If the state was still RUNNING detach from the EventBase.
    if (eventBase) {
      eventBase->runImmediatelyOrRunInEventBaseThreadAndWait([this] {
        {
          auto state = state_.wlock();
          state->conn_->clearReceiveCallback();
          state->conn_->detachEventBase();
        }
        cancel();
      });
    }
    // Make sure the socket is closed, and fail any outstanding requests.
    // Closing the socket will signal the privhelper process to exit.
    closeSocket(std::runtime_error("privhelper client being destroyed"));

    // Wait until the privhelper process exits.
    if (helperProc_.has_value()) {
      return folly::makeExpected<int>(helperProc_->wait());
    } else {
      // helperProc_ can be nullopt during the unit tests, where we aren't
      // actually running the privhelper in a separate process.
      return folly::makeExpected<int>(
          ProcessStatus(ProcessStatus::State::Exited, 0));
    }
  }

  /**
   * Send a request and wait for the response.
   */
  Future<UnixSocket::Message> sendAndRecv(
      uint32_t xid,
      UnixSocket::Message&& msg) {
    EventBase* eventBase;
    {
      auto state = state_.rlock();
      if (state->status != Status::RUNNING) {
        return folly::makeFuture<UnixSocket::Message>(std::runtime_error(
            "cannot send new requests on closed privhelper connection"));
      }
      eventBase = state->eventBase;
    }

    // Note: We intentionally use EventBase::runInEventBaseThread() here rather
    // than folly::via().
    //
    // folly::via() does not do what we want, as it causes chained futures to
    // use the original executor rather than to execute inline.  In particular
    // this causes problems during destruction if the EventBase in question has
    // already been destroyed.
    folly::Promise<UnixSocket::Message> promise;
    auto future = promise.getFuture();
    eventBase->runInEventBaseThread([this,
                                     xid,
                                     msg = std::move(msg),
                                     promise = std::move(promise)]() mutable {
      // Double check that the connection is still open
      {
        auto state = state_.rlock();
        if (!state->conn_) {
          promise.setException(std::runtime_error(
              "cannot send new requests on closed privhelper connection"));
          return;
        }
      }
      pendingRequests_.emplace(xid, std::move(promise));
      ++sendPending_;
      {
        auto state = state_.wlock();
        state->conn_->send(std::move(msg), this);
      }
    });
    return future;
  }

  void messageReceived(UnixSocket::Message&& message) noexcept override {
    try {
      processResponse(std::move(message));
    } catch (const std::exception& ex) {
      EDEN_BUG() << "unexpected error processing privhelper response: "
                 << folly::exceptionStr(ex);
    }
  }

  void processResponse(UnixSocket::Message&& message) {
    Cursor cursor(&message.data);
    PrivHelperConn::PrivHelperPacket packet =
        PrivHelperConn::parsePacket(cursor);

    auto iter = pendingRequests_.find(packet.metadata.transaction_id);
    if (iter == pendingRequests_.end()) {
      // This normally shouldn't happen unless there is a bug.
      // We'll throw and our caller will turn this into an EDEN_BUG()
      throwf<std::runtime_error>(
          "received unexpected response from privhelper for unknown transaction ID {}",
          packet.metadata.transaction_id);
    }

    auto promise = std::move(iter->second);
    pendingRequests_.erase(iter);
    promise.setValue(std::move(message));
  }

  void eofReceived() noexcept override {
    handleSocketError(std::runtime_error("privhelper process exited"));
  }

  void socketClosed() noexcept override {
    handleSocketError(
        std::runtime_error("privhelper client destroyed locally"));
  }

  void receiveError(const folly::exception_wrapper& ew) noexcept override {
    // Fail all pending requests
    handleSocketError(std::runtime_error(folly::to<string>(
        "error reading from privhelper process: ", folly::exceptionStr(ew))));
  }

  void sendSuccess() noexcept override {
    --sendPending_;
  }

  void sendError(const folly::exception_wrapper& ew) noexcept override {
    // Fail all pending requests
    --sendPending_;
    handleSocketError(std::runtime_error(folly::to<string>(
        "error sending to privhelper process: ", folly::exceptionStr(ew))));
  }

  void onEventBaseDestruction() noexcept override {
    // This callback is run when the EventBase is destroyed.
    // Detach from the EventBase.  We may be restarted later if
    // attachEventBase() is called again later to attach us to a new EventBase.
    detachWithinEventBaseDestructor();
  }

  void handleSocketError(const std::exception& ex) {
    // If we are RUNNING, move to the CLOSED state and then close the socket and
    // fail all pending requests.
    //
    // If we are in any other state just return early.
    // This can occur if handleSocketError() is invoked multiple times (e.g.,
    // for a send error and a receive error).  This can happen recursively since
    // closing the socket will generally trigger any outstanding sends and
    // receives to fail.
    {
      // Exit early if the state is not RUNNING.
      // Whatever other function updated the state will have handled closing the
      // socket and failing pending requests.
      auto state = state_.wlock();
      if (state->status != Status::RUNNING) {
        return;
      }
      state->status = Status::CLOSED;
      state->eventBase = nullptr;
    }
    closeSocket(ex);
  }

  void closeSocket(const std::exception& ex) {
    PendingRequestMap pending;
    pending.swap(pendingRequests_);
    {
      auto state = state_.wlock();
      state->conn_.reset();
    }
    XDCHECK_EQ(sendPending_, 0ul);

    for (auto& entry : pending) {
      entry.second.setException(ex);
    }
  }

  // Separated out from detachEventBase() since it is not safe to cancel() an
  // EventBase::OnDestructionCallback within the callback itself.
  void detachWithinEventBaseDestructor() noexcept {
    {
      auto state = state_.wlock();
      if (state->status != Status::RUNNING) {
        return;
      }
      state->status = Status::NOT_STARTED;
      state->eventBase = nullptr;
      state->conn_->clearReceiveCallback();
      state->conn_->detachEventBase();
    }
  }

  std::optional<SpawnedProcess> helperProc_;
  std::atomic<uint32_t> nextXid_{1};
  folly::Synchronized<ThreadSafeData> state_;

  // sendPending_, and pendingRequests_ are only accessed from the
  // EventBase thread.
  size_t sendPending_{0};
  PendingRequestMap pendingRequests_;
};

Future<File> PrivHelperClientImpl::fuseMount(
    StringPiece mountPath,
    bool readOnly) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeMountRequest(xid, mountPath, readOnly);
  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_MOUNT_FUSE, response);
        if (response.files.size() != 1) {
          throwf<std::runtime_error>(
              "expected privhelper FUSE response to contain a single file "
              "descriptor; got {}",
              response.files.size());
        }
        return std::move(response.files[0]);
      });
}

Future<Unit> PrivHelperClientImpl::nfsMount(
    folly::StringPiece mountPath,
    folly::SocketAddress mountdAddr,
    folly::SocketAddress nfsdAddr,
    bool readOnly,
    uint32_t iosize,
    bool useReaddirplus) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeMountNfsRequest(
      xid, mountPath, mountdAddr, nfsdAddr, readOnly, iosize, useReaddirplus);
  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_MOUNT_NFS, response);
      });
}

Future<Unit> PrivHelperClientImpl::fuseUnmount(StringPiece mountPath) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeUnmountRequest(xid, mountPath);
  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_UNMOUNT_FUSE, response);
      });
}

Future<Unit> PrivHelperClientImpl::nfsUnmount(StringPiece mountPath) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeNfsUnmountRequest(xid, mountPath);
  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_UNMOUNT_NFS, response);
      });
}

Future<Unit> PrivHelperClientImpl::bindMount(
    StringPiece clientPath,
    StringPiece mountPath) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeBindMountRequest(xid, clientPath, mountPath);

  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_MOUNT_BIND, response);
      });
}

folly::Future<folly::Unit> PrivHelperClientImpl::bindUnMount(
    folly::StringPiece mountPath) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeBindUnMountRequest(xid, mountPath);

  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_UNMOUNT_BIND, response);
      });
}

Future<Unit> PrivHelperClientImpl::takeoverShutdown(StringPiece mountPath) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeTakeoverShutdownRequest(xid, mountPath);

  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_TAKEOVER_SHUTDOWN, response);
      });
}

Future<Unit> PrivHelperClientImpl::takeoverStartup(
    StringPiece mountPath,
    const vector<string>& bindMounts) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeTakeoverStartupRequest(
      xid, mountPath, bindMounts);

  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_TAKEOVER_STARTUP, response);
      });
}

Future<Unit> PrivHelperClientImpl::setLogFile(folly::File logFile) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeSetLogFileRequest(xid, std::move(logFile));

  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_SET_LOG_FILE, response);
      });
}

Future<Unit> PrivHelperClientImpl::setDaemonTimeout(
    std::chrono::nanoseconds duration) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeSetDaemonTimeoutRequest(
      xid, std::move(duration));

  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_SET_DAEMON_TIMEOUT, response);
      });
}

Future<Unit> PrivHelperClientImpl::setUseEdenFs(bool useEdenFs) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeSetUseEdenFsRequest(xid, std::move(useEdenFs));

  return sendAndRecv(xid, std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_SET_USE_EDENFS, response);
      });
}

int PrivHelperClientImpl::stop() {
  const auto result = cleanup();
  if (result.hasError()) {
    folly::throwSystemErrorExplicit(
        result.error(), "error shutting down privhelper process");
  }
  auto status = result.value();
  if (status.killSignal() != 0) {
    return -status.killSignal();
  }
  return status.exitStatus();
}

bool PrivHelperClientImpl::checkConnection() {
  auto state = state_.rlock();
  return state->status == Status::RUNNING && state->conn_;
}

} // unnamed namespace

unique_ptr<PrivHelper>
startOrConnectToPrivHelper(const UserInfo& userInfo, int argc, char** argv) {
  std::string helperPathFromArgs;

  // We can't use FLAGS_ here because startOrConnectToPrivHelper() is called
  // before folly::init() and the args haven't been parsed yet. We do a very
  // simple iteration here to parse out the options.

  // But at least reference the symbol so it's included in the binary.
  void* volatile fd_arg = &FLAGS_privhelper_fd;
  (void)fd_arg;

  for (int i = 1; i < argc - 1; ++i) {
    StringPiece arg{argv[i]};
    if (arg == "--privhelper_fd") {
      // If EdenFS was passed the --privhelper_fd option (eg: by
      // daemonizeIfRequested) then it has a channel through which it can
      // communicate with a previously spawned privhelper process. Return a
      // client constructed from that channel.
      if ((i + 1) >= argc) {
        throw std::runtime_error("Too few arguments");
      }
      auto fdNum = folly::to<int>(argv[i + 1]);
      return make_unique<PrivHelperClientImpl>(
          folly::File(fdNum, true), std::nullopt);
    }

    if (arg == "--privhelper_path") {
      if ((i + 1) >= argc) {
        throw std::runtime_error("Too few arguments");
      }
      helperPathFromArgs = std::string(argv[i + 1]);
    }
  }

  SpawnedProcess::Options opts;

  // If EdenFS is running as setuid-root, it needs to be cautious about the
  // privhelper process that it's about start. Note: from a standard release
  // package, this is unlikely because the privhelper daemon is installed as
  // setuid-root and this allows us to avoid running the EdenFS executable as
  // setuid-root. All warnings will stay in the code since outside users should
  // be aware of the security implications of changing this code.
  //
  // This code require that both of these paths (the EdenFS exe and the
  // privhelper daemon) are not symlinks and that both are owned and controlled
  // by the same user (unless the privhelper daemon is owned by root).

  auto exePath = executablePath();
  auto canonPath = realpath(exePath.c_str());
  if (exePath != canonPath) {
    throwf<std::runtime_error>(
        "Refusing to start because my exePath {} is not the realpath to myself"
        " (which is {}). This is an unsafe installation and may be an"
        " indication of a symlink attack or similar attempt to escalate"
        " privileges.",
        exePath,
        canonPath);
  }

  bool isSetuid = getuid() != geteuid();

  AbsolutePath helperPath;

  // We should ALWAYS hit the first branch if running through official channels
  // (i.e. `eden start` and other internal methods), but there's a chance the
  // binary is invoked directly without --privhelper-path passed. In that case,
  // fall back to searching for a privhelper binary relative to the executable.
  if (!helperPathFromArgs.empty()) {
    if (isSetuid) {
      throw std::invalid_argument(
          "Cannot provide privhelper_path when executing a setuid binary");
    }
    helperPath = canonicalPath(helperPathFromArgs);
  } else {
    helperPath = exePath.dirname() + "edenfs_privhelper"_relpath;
  }
  XLOGF(DBG1, "Using '%s' as the privhelper daemon.\n", helperPath.c_str());

  struct stat helperStat {};
  struct stat selfStat {};

  checkUnixError(
      lstat(exePath.c_str(), &selfStat), fmt::format("lstat {}", exePath));
  checkUnixError(
      lstat(helperPath.c_str(), &helperStat),
      fmt::format("lstat {}", helperPath));

  if (isSetuid) {
    // Note: In a standard release package, the privhelper daemon is setuid-root
    // and the EdenFS executable is NOT. Therefore, the following is an unlikely
    // scenario. This comment/code is a warning to anyone who modifies this code
    // that there are major risks if shipping/running the EdenFS daemon as
    // setuid-root.
    //
    // When the EdenFS executable is a setuid binary: Require that our
    // executable be owned by root, otherwise refuse to continue on the basis
    // that something is very fishy.
    if (selfStat.st_uid != 0) {
      throwf<std::runtime_error>(
          "Refusing to start because my exePath {} is owned by uid {} rather"
          " than by root.",
          exePath,
          selfStat.st_uid);
    }
  }

  // This is not a concern if the privhelper is setuid-root. At that point,
  // there are bigger concerns than our uid/gid not matching. In addition, we
  // want dev EdenFS instances to be able to use system (setuid-root) privhelper
  // binaries while being run as a non-root user.
  if ((helperStat.st_uid != 0 && (selfStat.st_uid != helperStat.st_uid)) ||
      (helperStat.st_gid != 0 && (selfStat.st_gid != helperStat.st_gid))) {
    throwf<std::runtime_error>(
        "Refusing to start because my exePath {} is owned by uid={} gid={} and"
        " that doesn't match the ownership of {} which is owned by uid={}"
        " gid={}",
        exePath,
        selfStat.st_uid,
        selfStat.st_gid,
        helperPath,
        helperStat.st_uid,
        helperStat.st_gid);
  }

  if (S_ISLNK(helperStat.st_mode)) {
    throwf<std::runtime_error>(
        "Refusing to start because {} is a symlink", helperPath);
  }

  opts.executablePath(helperPath);

  File clientConn;
  File serverConn;
  PrivHelperConn::createConnPair(clientConn, serverConn);
  auto control = opts.inheritDescriptor(
      FileDescriptor(serverConn.release(), FileDescriptor::FDType::Socket));
  SpawnedProcess proc(
      {
          "edenfs_privhelper",
          // pass down identity information.
          folly::to<std::string>("--privhelper_uid=", userInfo.getUid()),
          folly::to<std::string>("--privhelper_gid=", userInfo.getGid()),
          // pass down the control pipe
          folly::to<std::string>("--privhelper_fd=", control),
      },
      std::move(opts));

  XLOG(DBG1) << "Spawned mount helper process: pid=" << proc.pid();
  return make_unique<PrivHelperClientImpl>(
      std::move(clientConn), std::move(proc));
}

unique_ptr<PrivHelper> createTestPrivHelper(File&& conn) {
  return make_unique<PrivHelperClientImpl>(std::move(conn), std::nullopt);
}

#else // _WIN32

unique_ptr<PrivHelper>
startOrConnectToPrivHelper(const UserInfo&, int, char**) {
  return make_unique<PrivHelper>();
}

#endif // _WIN32

} // namespace facebook::eden
