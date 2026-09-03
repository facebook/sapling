/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/privhelper/PrivHelperImpl.h"

#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "eden/fs/telemetry/LogEvent.h"

#ifndef _WIN32
#include <sysexits.h>
#endif

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/SocketAddress.h>
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/async/AsyncTimeout.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Fcntl.h>
#include <folly/portability/SysTypes.h>
#include <folly/portability/Unistd.h>

#include "eden/common/utils/Bug.h"
#include "eden/common/utils/FileDescriptor.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/SpawnedProcess.h"
#include "eden/common/utils/UserInfo.h"
#include "eden/fs/privhelper/PrivHelper.h"
#include "eden/fs/privhelper/PrivHelperConn.h"
#include "eden/fs/privhelper/PrivHelperFlags.h"
#include "eden/fs/utils/NotImplemented.h"

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
 * The privhelper connection: the socket, the state guarding it, and every
 * callback the UnixSocket and the EventBase hold a pointer to.
 *
 * Held by shared_ptr, and work posted to the EventBase captures a share.
 * Nothing joins that work, so the session outlives the caller's PrivHelper
 * whenever a task is still queued against it.
 */
class PrivHelperClientSession
    : public std::enable_shared_from_this<PrivHelperClientSession>,
      private UnixSocket::ReceiveCallback,
      private UnixSocket::SendCallback,
      private EventBase::OnDestructionCallback {
 public:
  explicit PrivHelperClientSession(File conn)
      : state_{ThreadSafeData{
            Status::NOT_ATTACHED,
            nullptr,
            UnixSocket::makeUnique(nullptr, std::move(conn))}} {}

  ~PrivHelperClientSession() override {
    XDCHECK_EQ(sendPending_, 0ul);
  }

  void attachEventBase(EventBase* eventBase) {
    {
      auto state = state_.wlock();
      if (state->status != Status::NOT_ATTACHED) {
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

  void detachEventBase() {
    detachWithinEventBaseDestructor();
    cancel();
  }

  bool checkConnection() {
    auto state = state_.rlock();
    return state->status == Status::RUNNING && state->conn_;
  }

  /**
   * Set the logger used for privhelper telemetry events. Must be called before
   * attachEventBase(); read on the EventBase thread thereafter.
   */
  void setEdenFsEventsLogger(std::shared_ptr<EdenFsEventsLogger> logger) {
    edenFsEventsLogger_ = std::move(logger);
  }

  /**
   * Override the stall-report threshold. May only be called before requests
   * are issued.
   */
  void setRequestStallThreshold(std::chrono::milliseconds threshold) {
    requestStallThreshold_ = threshold;
  }

  int getRawClientFd() const {
    auto state = state_.rlock();
    return state->conn_ ? state->conn_->getRawFd() : -1;
  }

  std::shared_ptr<EdenFsEventsLogger> getEdenFsEventsLogger() const {
    return edenFsEventsLogger_;
  }

  uint32_t getNextXid() {
    return nextXid_.fetch_add(1, std::memory_order_acq_rel);
  }

  /**
   * Close the socket to the privhelper server and fail outstanding requests.
   *
   * Returns false if the session was already shut down.
   */
  bool shutdown() {
    EventBase* eventBase{nullptr};
    {
      auto state = state_.wlock();
      if (state->status == Status::SHUT_DOWN) {
        return false;
      }
      if (state->status == Status::RUNNING) {
        eventBase = state->eventBase;
        state->eventBase = nullptr;
      }
      state->status = Status::SHUT_DOWN;
    }

    // If the state was still RUNNING detach from the EventBase.
    if (eventBase) {
      eventBase->runImmediatelyOrRunInEventBaseThreadAndWait([this] {
        {
          auto state = state_.wlock();
          state->conn_->clearReceiveCallback();
          state->conn_->detachEventBase();
        }
        // Cancel stall watchdogs while still on the EventBase thread, since
        // closeSocket() below destroys the pending request map from this
        // thread.
        cancelStallWatchdogs();
        cancel();
      });
    }
    // Make sure the socket is closed, and fail any outstanding requests.
    // Closing the socket will signal the privhelper process to exit.
    closeSocket(
        folly::make_exception_wrapper<std::runtime_error>(
            "privhelper client being destroyed"));
    return true;
  }

  /**
   * Send a request and wait for the response.
   *
   * A watchdog logs requests that stay pending longer than
   * requestStallThreshold_. It is log-only: a stalled request is never
   * failed, cancelled, or timed out.
   */
  Future<UnixSocket::Message> sendAndRecv(
      uint32_t xid,
      folly::StringPiece kind,
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
    eventBase->runInEventBaseThread([self = shared_from_this(),
                                     xid,
                                     kind,
                                     msg = std::move(msg),
                                     promise = std::move(promise),
                                     eventBase]() mutable {
      // Double check that the connection is still open, and only hold the
      // lock to look up the connection: send() can fail synchronously and
      // invoke the error callbacks, which re-enter handleSocketError() and
      // acquire state_ again, deadlocking this EventBase thread if the
      // lock were still held.
      //
      // The status re-check also prevents arming a watchdog after shutdown()
      // has run its one-and-only cancelStallWatchdogs() pass on this thread.
      // conn_ can still be non-null at that point (detached but not yet
      // closed), and closeSocket() could otherwise destroy the watchdog off
      // the EventBase thread.
      //
      // Using the raw pointer after releasing the lock is safe: conn_ is
      // only mutated on this EventBase thread, except in cleanup(), which
      // first moves the status off RUNNING and then drains this EventBase.
      // Any lambda like this one that was enqueued before the status
      // change is ordered before cleanup()'s drain on this thread, so it
      // runs while the socket is still alive; lambdas enqueued afterwards
      // never pass the status check above.
      UnixSocket* conn = nullptr;
      {
        auto state = self->state_.rlock();
        if (state->status != Status::RUNNING || !state->conn_) {
          promise.setException(
              std::runtime_error(
                  "cannot send new requests on closed privhelper connection"));
          return;
        }
        conn = state->conn_.get();
      }
      // The watchdog captures a raw session pointer rather than a shared_ptr:
      // it is owned by the session (via pendingRequests_) so it cannot outlive
      // it, and a shared_ptr capture would form a reference cycle.
      auto stallWatchdog = folly::AsyncTimeout::make(
          *eventBase, [session = self.get(), xid]() noexcept {
            session->requestStalled(xid);
          });
      stallWatchdog->scheduleTimeout(self->requestStallThreshold_);
      self->pendingRequests_.emplace(
          xid,
          PendingRequest{
              std::move(promise),
              kind,
              std::chrono::steady_clock::now(),
              std::move(stallWatchdog)});
      ++self->sendPending_;
      conn->send(std::move(msg), self.get());
    });
    return future;
  }

  /**
   * Send a request without waiting for a response.
   *
   * The message is only enqueued: there is no way to force the write out
   * without aborting or racing UnixSocket's own queued writes, so it races the
   * socket closing and may never reach the server.
   */
  void sendOneWay(UnixSocket::Message&& msg) {
    EventBase* eventBase;
    {
      auto state = state_.rlock();
      if (state->status != Status::RUNNING) {
        return;
      }
      eventBase = state->eventBase;
    }
    if (!eventBase) {
      return;
    }

    eventBase->runInEventBaseThread(
        [self = shared_from_this(), msg = std::move(msg)]() mutable {
          auto state = self->state_.wlock();
          if (!state->conn_) {
            return;
          }
          // The null send callback is load-bearing: this send can still be
          // queued while closeSocket() holds state_'s write lock, and a
          // callback would reacquire it via sendError(); folly::Synchronized
          // is not recursive.
          state->conn_->send(std::move(msg));
        });
  }

 private:
  struct PendingRequest {
    folly::Promise<UnixSocket::Message> promise;
    // Points at a string literal (static storage duration) passed to
    // sendAndRecv; no copy needed.
    folly::StringPiece kind;
    std::chrono::steady_clock::time_point startTime;
    // Cancels the stall watchdog when destroyed. A scheduled AsyncTimeout
    // may only be destroyed on the EventBase thread; every detach path
    // flips status off RUNNING and then cancels pending watchdogs there
    // (cancelStallWatchdogs) before the map can be destroyed from another
    // thread. sendAndRecv's EventBase-thread lambda re-checks status so no
    // new watchdog can be armed after that cancel pass.
    std::unique_ptr<folly::AsyncTimeout> stallWatchdog;
    bool stalled{false};
  };
  using PendingRequestMap = std::unordered_map<uint32_t, PendingRequest>;
  enum class Status : uint32_t {
    /**
     * Socket open, not on an EventBase: either never attached, or detached
     * again when one was destroyed. The only state attachEventBase() accepts.
     */
    NOT_ATTACHED,
    /** Socket open and attached. The only state that accepts new requests. */
    RUNNING,
    /**
     * The socket went away and has been released: EOF, a send or receive
     * error, or a local close. shutdown() has not run, so the process still
     * needs reaping.
     */
    DISCONNECTED,
    /** shutdown() has run: nothing left to release, and no process to reap. */
    SHUT_DOWN,
  };
  struct ThreadSafeData {
    Status status;
    EventBase* eventBase;
    UnixSocket::UniquePtr conn_;
  };

  // Runs on the EventBase thread when a request has been pending longer than
  // requestStallThreshold_. Log-only: the request is left untouched.
  void requestStalled(uint32_t xid) {
    auto iter = pendingRequests_.find(xid);
    if (iter == pendingRequests_.end()) {
      return;
    }
    auto& request = iter->second;
    request.stalled = true;
    const auto elapsed = std::chrono::duration<double>(
        std::chrono::steady_clock::now() - request.startTime);
    XLOGF(
        WARN,
        "privhelper {} request (txid {}) still pending after {:.1f}s; "
        "the privhelper may be wedged",
        request.kind,
        xid,
        elapsed.count());
    if (edenFsEventsLogger_) {
      try {
        edenFsEventsLogger_->logEvent(
            PrivhelperRequestStall{request.kind.str(), elapsed.count()});
      } catch (const std::exception& ex) {
        XLOGF(
            WARN,
            "failed to log privhelper_request_stall event: {}",
            ex.what());
      }
    }
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

    auto request = std::move(iter->second);
    pendingRequests_.erase(iter);
    if (request.stalled) {
      const auto elapsed = std::chrono::duration<double>(
          std::chrono::steady_clock::now() - request.startTime);
      XLOGF(
          WARN,
          "stalled privhelper {} request (txid {}) eventually completed "
          "after {:.1f}s",
          request.kind,
          packet.metadata.transaction_id,
          elapsed.count());
    }
    request.promise.setValue(std::move(message));
  }

  void eofReceived() noexcept override {
    handleSocketError(
        "eof",
        folly::make_exception_wrapper<std::runtime_error>(
            "privhelper process exited"));
  }

  void socketClosed() noexcept override {
    handleSocketError(
        "socket_closed",
        folly::make_exception_wrapper<std::runtime_error>(
            "privhelper client destroyed locally"));
  }

  void receiveError(const folly::exception_wrapper& ew) noexcept override {
    // Fail all pending requests
    handleSocketError(
        "receive_error",
        folly::make_exception_wrapper<std::runtime_error>(folly::to<string>(
            "error reading from privhelper process: ",
            folly::exceptionStr(ew))));
  }

  void sendSuccess() noexcept override {
    --sendPending_;
  }

  void sendError(const folly::exception_wrapper& ew) noexcept override {
    // Fail all pending requests
    --sendPending_;
    handleSocketError(
        "send_error",
        folly::make_exception_wrapper<std::runtime_error>(folly::to<string>(
            "error sending to privhelper process: ", folly::exceptionStr(ew))));
  }

  void onEventBaseDestruction() noexcept override {
    // This callback is run when the EventBase is destroyed.
    // Detach from the EventBase.  We may be restarted later if
    // attachEventBase() is called again later to attach us to a new EventBase.
    detachWithinEventBaseDestructor();
  }

  void handleSocketError(
      folly::StringPiece reason,
      const folly::exception_wrapper& ew) {
    // If we are RUNNING, move to the DISCONNECTED state and then close the
    // socket and fail all pending requests.
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
      state->status = Status::DISCONNECTED;
      state->eventBase = nullptr;
    }
    XLOG(ERR) << "lost connection to privhelper process (" << reason
              << "): " << folly::exceptionStr(ew);
    if (edenFsEventsLogger_) {
      edenFsEventsLogger_->logEvent(PrivHelperExit{reason.str()});
    }
    closeSocket(ew);
    // The EventBase is no longer in use; without this, destroying the
    // client later fails OnDestructionCallback's must-be-canceled check.
    cancel();
  }

  /**
   * Tear down the connection and fail all pending requests.
   *
   * Safe to call from inside the socket's own callbacks:
   * UnixSocket::destroy() defers its teardown until the callback stack
   * unwinds.
   */
  void closeSocket(const folly::exception_wrapper& ew) {
    PendingRequestMap pending;
    pending.swap(pendingRequests_);
    // Move the socket out of state_ and destroy it only after releasing the
    // lock: if a receive callback is still registered (the EOF and error
    // paths), destroying the socket synchronously invokes socketClosed(),
    // which re-enters handleSocketError() and acquires state_ again.
    // folly::SharedMutex is not reentrant, so destroying the socket while
    // holding the write lock deadlocks the EventBase thread, silently
    // hanging every future privhelper request.
    UnixSocket::UniquePtr conn;
    {
      auto state = state_.wlock();
      conn = std::move(state->conn_);
    }
    conn.reset();

    for (auto& entry : pending) {
      entry.second.promise.setException(ew);
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
      state->status = Status::NOT_ATTACHED;
      state->eventBase = nullptr;
      state->conn_->clearReceiveCallback();
      state->conn_->detachEventBase();
    }
    cancelStallWatchdogs();
  }

  // Must run on the EventBase thread (or during EventBase destruction):
  // a scheduled AsyncTimeout may only be cancelled there.
  void cancelStallWatchdogs() noexcept {
    for (auto& entry : pendingRequests_) {
      entry.second.stallWatchdog.reset();
    }
  }

  std::atomic<uint32_t> nextXid_{1};
  folly::Synchronized<ThreadSafeData> state_;
  // Must be set (via setEdenFsEventsLogger) before attachEventBase() is called.
  // Read from EventBase thread thereafter; do not modify after attach.
  std::shared_ptr<EdenFsEventsLogger> edenFsEventsLogger_;
  // Pending requests are reported as stalled (log-only) after this long.
  // May only be modified before requests are issued; read on the EventBase
  // thread.
  std::chrono::milliseconds requestStallThreshold_{std::chrono::minutes(1)};

  // sendPending_, and pendingRequests_ are only accessed from the
  // EventBase thread.
  size_t sendPending_{0};
  PendingRequestMap pendingRequests_;
};

/**
 * PrivHelperClientImpl contains the client-side logic (in the parent process)
 * for talking to the remote privileged process.
 */
class PrivHelperClientImpl : public PrivHelper {
 public:
  PrivHelperClientImpl(File conn, std::optional<SpawnedProcess> proc)
      : helperProc_(std::move(proc)),
        session_(std::make_shared<PrivHelperClientSession>(std::move(conn))) {
    pid_ = -1;
    if (helperProc_.has_value()) {
      pid_ = helperProc_->pid();
    }
    // If we need to get the pid from the server, we need to
    // wait until the connection is started
  }
  ~PrivHelperClientImpl() override {
    if (session_->shutdown()) {
      waitForHelperProcess();
    }
  }

  void attachEventBase(EventBase* eventBase) override {
    session_->attachEventBase(eventBase);
  }

  void detachEventBase() override {
    session_->detachEventBase();
  }

  Future<File> fuseMount(
      folly::StringPiece mountPath,
      bool readOnly,
      StringPiece vfsType) override;
  Future<Unit> fuseUnmount(StringPiece mountPath, const UnmountOptions& options)
      override;
  Future<Unit> nfsMount(
      folly::StringPiece mountPath,
      const NFSMountOptions& options) override;
  Future<Unit> nfsUnmount(StringPiece mountPath) override;
  Future<Unit> bindMount(StringPiece clientPath, StringPiece mountPath)
      override;
  folly::Future<folly::Unit> bindUnMount(folly::StringPiece mountPath) override;
  Future<Unit> takeoverShutdown(StringPiece mountPath) override;
  Future<Unit> takeoverStartup(
      StringPiece mountPath,
      const vector<string>& bindMounts) override;
  Future<Unit> setLogFile(folly::File logFile) override;
  Future<pid_t> getServerPid() override;
  Future<NamespaceInfo> getNamespaceInfo(pid_t daemonPid) override;
  Future<pid_t> startFam(
      const std::vector<std::string>& paths,
      const std::string& tmpOutputPath,
      const std::string& specifiedOutputPath,
      const bool shouldUpload) override;
  Future<StopFileAccessMonitorResponse> stopFam() override;
  Future<folly::Unit> setMemoryPriorityForProcess(pid_t pid, int priority)
      override;
  Future<folly::Unit> setFuseReadAhead(
      StringPiece mountPath,
      uint32_t readAheadKb) override;
  Future<Unit> setRestartArgs(const EdenFsRestartArgs& args) override;
  void notifyCleanShutdown(StringPiece reason) noexcept override;
  void setEdenFsEventsLogger(
      std::shared_ptr<EdenFsEventsLogger> logger) override {
    session_->setEdenFsEventsLogger(std::move(logger));
  }
  void setRequestStallThresholdForTest(
      std::chrono::milliseconds threshold) override {
    session_->setRequestStallThreshold(threshold);
  }
  int stop() override;
  int getRawClientFd() const override {
    return session_->getRawClientFd();
  }
  bool checkConnection() override {
    return session_->checkConnection();
  }
  int getPid() override;

 private:
  uint32_t getNextXid() {
    return session_->getNextXid();
  }

  Future<UnixSocket::Message> sendAndRecv(
      uint32_t xid,
      folly::StringPiece kind,
      UnixSocket::Message&& msg) {
    return session_->sendAndRecv(xid, kind, std::move(msg));
  }

  ProcessStatus waitForHelperProcess() {
    if (helperProc_.has_value()) {
      return helperProc_->wait();
    }
    // helperProc_ can be nullopt during the unit tests, where we aren't
    // actually running the privhelper in a separate process.
    return ProcessStatus(ProcessStatus::State::Exited, 0);
  }

  std::optional<SpawnedProcess> helperProc_;
  pid_t pid_;
  const std::shared_ptr<PrivHelperClientSession> session_;
};

/**
 * Parse sanity-check results from a privhelper response and log a
 * StaleRedirectionCleanup event when stale mounts were found.
 *
 * Best-effort: parsing or logging failures are caught so that telemetry
 * never breaks mount/takeover operations.
 *
 * TODO: The response packet header is parsed twice (once by
 * parseEmptyResponse and once here). Consider refactoring
 * parseEmptyResponse to return a Cursor positioned after the header.
 */
void logSanityCheckResult(
    const std::shared_ptr<EdenFsEventsLogger>& logger,
    const UnixSocket::Message& response,
    const std::string& mountPath) {
  try {
    Cursor cursor(&response.data);
    PrivHelperConn::parsePacket(cursor);
    auto sanityResult = PrivHelperConn::parseSanityCheckResult(cursor);

    if (logger &&
        (sanityResult.staleRedirectionMountsFound > 0 ||
         sanityResult.staleCheckoutMountUnmounted)) {
      logger->logEvent(
          StaleRedirectionCleanup{
              mountPath,
              sanityResult.staleRedirectionMountsFound,
              sanityResult.staleRedirectionMountsSucceeded,
              sanityResult.staleRedirectionMountsFailed,
              sanityResult.staleCheckoutMountUnmounted});
    }
  } catch (const std::exception& ex) {
    XLOGF(
        WARN,
        "Failed to parse sanity check result for {}: {}",
        mountPath,
        ex.what());
  }
}

Future<File> PrivHelperClientImpl::fuseMount(
    StringPiece mountPath,
    bool readOnly,
    StringPiece vfsType) {
  auto xid = getNextXid();
  auto mountPathStr = mountPath.str();
  auto request =
      PrivHelperConn::serializeMountRequest(xid, mountPath, readOnly, vfsType);
  return sendAndRecv(xid, "fuse_mount", std::move(request))
      .thenValue(
          [mountPathStr = std::move(mountPathStr),
           logger = session_->getEdenFsEventsLogger()](
              UnixSocket::Message&& response)
              -> folly::Future<UnixSocket::Message> {
            PrivHelperConn::parseEmptyResponse(
                PrivHelperConn::REQ_MOUNT_FUSE, response);
            logSanityCheckResult(logger, response, mountPathStr);
            return std::move(response);
          })
      .thenValue([](UnixSocket::Message&& response) {
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
    const NFSMountOptions& options) {
  auto xid = getNextXid();
  auto mountPathStr = mountPath.str();
  auto request =
      PrivHelperConn::serializeMountNfsRequest(xid, mountPath, options);

  return sendAndRecv(xid, "nfs_mount", std::move(request))
      .thenValue(
          [mountPathStr = std::move(mountPathStr),
           logger = session_->getEdenFsEventsLogger()](
              UnixSocket::Message&& response) mutable -> Future<Unit> {
            PrivHelperConn::parseEmptyResponse(
                PrivHelperConn::REQ_MOUNT_NFS, response);
            logSanityCheckResult(logger, response, mountPathStr);
            return folly::unit;
          });
}

Future<Unit> PrivHelperClientImpl::fuseUnmount(
    StringPiece mountPath,
    const UnmountOptions& options) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeUnmountRequest(xid, mountPath, options);

  return sendAndRecv(xid, "fuse_unmount", std::move(request))
      .thenValue([](UnixSocket::Message&& response) mutable -> Future<Unit> {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_UNMOUNT_FUSE, response);
        return folly::unit;
      });
}

Future<Unit> PrivHelperClientImpl::nfsUnmount(StringPiece mountPath) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeNfsUnmountRequest(xid, mountPath);
  return sendAndRecv(xid, "nfs_unmount", std::move(request))
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

  return sendAndRecv(xid, "bind_mount", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_MOUNT_BIND, response);
      });
}

folly::Future<folly::Unit> PrivHelperClientImpl::bindUnMount(
    folly::StringPiece mountPath) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeBindUnMountRequest(xid, mountPath);

  return sendAndRecv(xid, "bind_unmount", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_UNMOUNT_BIND, response);
      });
}

Future<Unit> PrivHelperClientImpl::takeoverShutdown(StringPiece mountPath) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeTakeoverShutdownRequest(xid, mountPath);

  return sendAndRecv(xid, "takeover_shutdown", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_TAKEOVER_SHUTDOWN, response);
      });
}

Future<Unit> PrivHelperClientImpl::takeoverStartup(
    StringPiece mountPath,
    const vector<string>& bindMounts) {
  auto xid = getNextXid();
  auto mountPathStr = mountPath.str();
  auto request = PrivHelperConn::serializeTakeoverStartupRequest(
      xid, mountPath, bindMounts);

  return sendAndRecv(xid, "takeover_startup", std::move(request))
      .thenValue([mountPathStr = std::move(mountPathStr),
                  logger = session_->getEdenFsEventsLogger()](
                     UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_TAKEOVER_STARTUP, response);
        logSanityCheckResult(logger, response, mountPathStr);
      });
}

Future<Unit> PrivHelperClientImpl::setLogFile(folly::File logFile) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeSetLogFileRequest(xid, std::move(logFile));

  return sendAndRecv(xid, "set_log_file", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_SET_LOG_FILE, response);
      });
}

Future<pid_t> PrivHelperClientImpl::getServerPid() {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeGetPidRequest(xid);

  return sendAndRecv(xid, "get_pid", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        return PrivHelperConn::parseGetPidResponse(response);
      });
}

Future<NamespaceInfo> PrivHelperClientImpl::getNamespaceInfo(pid_t daemonPid) {
  auto xid = getNextXid();
  auto request =
      PrivHelperConn::serializeGetNamespaceInfoRequest(xid, daemonPid);

  return sendAndRecv(xid, "get_namespace_info", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        return PrivHelperConn::parseGetNamespaceInfoResponse(response);
      });
}

Future<pid_t> PrivHelperClientImpl::startFam(
    const std::vector<std::string>& paths,
    const std::string& tmpOutputPath,
    const std::string& specifiedOutputPath,
    const bool shouldUpload) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeStartFamRequest(
      xid, paths, tmpOutputPath, specifiedOutputPath, shouldUpload);

  return sendAndRecv(xid, "start_fam", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        return PrivHelperConn::parseStartFamResponse(response);
      });
}

Future<StopFileAccessMonitorResponse> PrivHelperClientImpl::stopFam() {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeStopFamRequest(xid);

  return sendAndRecv(xid, "stop_fam", std::move(request))
      .thenValue([&](UnixSocket::Message&& response) {
        StopFileAccessMonitorResponse stopResponse{};
        PrivHelperConn::parseStopFamResponse(
            response,
            stopResponse.tmpOutputPath,
            stopResponse.specifiedOutputPath,
            stopResponse.shouldUpload);
        return stopResponse;
      });
}

Future<Unit> PrivHelperClientImpl::setMemoryPriorityForProcess(
    pid_t pid,
    int priority) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeSetMemoryPriorityForProcessRequest(
      xid, pid, priority);

  return sendAndRecv(xid, "set_memory_priority", std::move(request))
      .thenValue([pid, priority](UnixSocket::Message&& response) {
        try {
          PrivHelperConn::parseEmptyResponse(
              PrivHelperConn::REQ_SET_MEMORY_PRIORITY_FOR_PROCESS, response);
        } catch (const PrivHelperError& e) {
          // If the unmount failed, it likely means we are communicating
          // with a PrivHelper server that doesn't understand how to set memory
          // priority. Ignore the error for now.
          // TODO[T214491519] remove this after 1-2 months.
          XLOGF(
              ERR,
              "Failed to set memory priority to {} for process {}: {}",
              priority,
              pid,
              e.what());
        }
        return folly::unit;
      });
}

Future<Unit> PrivHelperClientImpl::setFuseReadAhead(
    StringPiece mountPath,
    uint32_t readAheadKb) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeSetFuseReadAheadRequest(
      xid, mountPath, readAheadKb);
  return sendAndRecv(xid, "set_fuse_read_ahead", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_SET_FUSE_READ_AHEAD, response);
      });
}

Future<Unit> PrivHelperClientImpl::setRestartArgs(
    const EdenFsRestartArgs& args) {
  auto xid = getNextXid();
  auto request = PrivHelperConn::serializeSetRestartArgsRequest(xid, args);

  return sendAndRecv(xid, "set_restart_args", std::move(request))
      .thenValue([](UnixSocket::Message&& response) {
        PrivHelperConn::parseEmptyResponse(
            PrivHelperConn::REQ_SET_RESTART_ARGS, response);
      });
}

void PrivHelperClientImpl::notifyCleanShutdown(StringPiece reason) noexcept {
  // Best effort: this runs on the shutdown path, where the message races the
  // EOF and the server may never see it.
  session_->sendOneWay(
      PrivHelperConn::serializeNotifyCleanShutdownRequest(
          getNextXid(), reason));
}

int PrivHelperClientImpl::stop() {
  if (!session_->shutdown()) {
    // Already torn down, so there is no process left to wait for.
    folly::throwSystemErrorExplicit(
        ESRCH, "error shutting down privhelper process");
  }
  const auto status = waitForHelperProcess();
  if (status.killSignal() != 0) {
    return -status.killSignal();
  }
  return status.exitStatus();
}

int PrivHelperClientImpl::getPid() {
  if (pid_ == -1 && checkConnection()) {
    // Get pid from server after connection is made
    try {
      pid_ = getServerPid().get();
    } catch (const facebook::eden::PrivHelperError& ex) {
      XLOGF(ERR, "Failed to get pid from privhelper: {}", ex.what());
      return -1;
    }
  }
  return pid_;
}

} // unnamed namespace

bool tccDisclaimKillswitchPresent(const char* path) {
  return access(path, F_OK) == 0;
}

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
      // This descriptor crossed an exec, so it cannot have arrived
      // close-on-exec. Without FD_CLOEXEC it leaks into every process EdenFS
      // spawns, and the privhelper then sees no EOF when EdenFS dies.
      folly::checkUnixError(
          fcntl(fdNum, F_SETFD, FD_CLOEXEC),
          "failed to set FD_CLOEXEC on the privhelper client descriptor");
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

#ifdef __APPLE__
  if (tccDisclaimKillswitchPresent()) {
    XLOGF(
        INFO,
        "not disclaiming TCC responsibility for the privhelper: killswitch "
        "file {} is present",
        kTccDisclaimKillswitchPath);
  } else {
    // Make the privhelper its own TCC responsible process so that TCC grants
    // keyed to its code signature apply regardless of what launched EdenFS.
    opts.disclaimTccResponsibility();
  }
#endif

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

  struct stat helperStat{};
  struct stat selfStat{};

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
  try {
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

    XLOGF(DBG1, "Spawned mount helper process: pid={}", proc.pid());
    return make_unique<PrivHelperClientImpl>(
        std::move(clientConn), std::move(proc));
  } catch (const std::system_error& ex) {
    if (ex.code().value() == EPERM) {
      XLOG(
          ERR,
          "error starting EdenFS: could not start privhelper process. "
          "This can happen when EdenFS is started in an environment "
          "that does not allow to launch privileged processes.");
      _exit(EX_NOPERM);
    }
    throw;
  }
}

unique_ptr<PrivHelper> createTestPrivHelper(File conn) {
  return make_unique<PrivHelperClientImpl>(std::move(conn), std::nullopt);
}

#else // _WIN32

namespace {

/**
 * A stub PrivHelper class for Windows.
 *
 * We do not actually use a separate privhelper process on Windows. However,
 * for ease of sharing server initialization code across platforms, we still
 * define a PrivHelper object, but it does nothing.
 *
 * Unsupported operations throw NOT_IMPLEMENTED.
 */
class StubPrivHelper final : public PrivHelper {
 public:
  void attachEventBase(folly::EventBase* eventBase) override {
    (void)eventBase;
  }

  void detachEventBase() override {}

  folly::Future<folly::File> fuseMount(
      folly::StringPiece mountPath,
      bool readOnly,
      folly::StringPiece vfsType) override {
    (void)mountPath;
    (void)readOnly;
    (void)vfsType;
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> nfsMount(
      folly::StringPiece mountPath,
      const NFSMountOptions& options) override {
    (void)mountPath;
    (void)options;
    // TODO: We do support NFS on Windows. Should the mount flow be
    // implemented here?
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> fuseUnmount(
      folly::StringPiece mountPath,
      const UnmountOptions& options) override {
    (void)mountPath;
    (void)options;
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> nfsUnmount(folly::StringPiece mountPath) override {
    (void)mountPath;
    // TODO: We do support NFS on Windows. Should the mount flow be
    // implemented here?
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> bindMount(
      folly::StringPiece clientPath,
      folly::StringPiece mountPath) override {
    (void)clientPath;
    (void)mountPath;
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> bindUnMount(
      folly::StringPiece mountPath) override {
    (void)mountPath;
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> takeoverShutdown(
      folly::StringPiece mountPath) override {
    (void)mountPath;
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> takeoverStartup(
      folly::StringPiece mountPath,
      const std::vector<std::string>& bindMounts) override {
    (void)mountPath;
    (void)bindMounts;
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> setLogFile(folly::File logFile) override {
    (void)logFile;
    return folly::unit;
  }

  folly::Future<pid_t> getServerPid() override {
    return -1;
  }

  Future<NamespaceInfo> getNamespaceInfo(pid_t daemonPid) override {
    (void)daemonPid;
    NOT_IMPLEMENTED();
  }

  folly::Future<pid_t> startFam(
      const std::vector<std::string>& paths,
      const std::string& tmpOutputPath,
      const std::string& specifiedOutputPath,
      const bool shouldUpload) override {
    (void)paths;
    (void)tmpOutputPath;
    (void)specifiedOutputPath;
    (void)shouldUpload;
    NOT_IMPLEMENTED();
  }

  folly::Future<StopFileAccessMonitorResponse> stopFam() override {
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> setMemoryPriorityForProcess(
      pid_t pid,
      int priority) override {
    (void)pid;
    (void)priority;
    NOT_IMPLEMENTED();
  }

  folly::Future<folly::Unit> setFuseReadAhead(
      folly::StringPiece mountPath,
      uint32_t readAheadKb) override {
    (void)mountPath;
    (void)readAheadKb;
    NOT_IMPLEMENTED();
  }

  int stop() override {
    return 0;
  }

  int getRawClientFd() const override {
    NOT_IMPLEMENTED();
  }

  bool checkConnection() override {
    // checkConnection() is used to determine whether the privhelper is healthy
    // in `eden doctor`. The Windows privhelper stub is always healthy, so
    // return true.
    return true;
  }

  int getPid() override {
    // Since we don't have a privhelper process return -1 to mark no process
    return -1;
  }
};

} // namespace

unique_ptr<PrivHelper>
startOrConnectToPrivHelper(const UserInfo&, int, char**) {
  return make_unique<StubPrivHelper>();
}

#endif // _WIN32

} // namespace facebook::eden
