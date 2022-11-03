/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

// Implementation of the NFSv3 protocol as described in:
// https://tools.ietf.org/html/rfc1813

#include "eden/fs/nfs/NfsDispatcher.h"
#include "eden/fs/nfs/rpc/Server.h"
#include "eden/fs/telemetry/TraceBus.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/ProcessAccessLog.h"

namespace folly {
class Executor;
}

namespace facebook::eden {

class Notifier;
class ProcessNameCache;
class FsEventLogger;
class StructuredLogger;

using TraceDetailedArgumentsHandle = std::shared_ptr<void>;

struct NfsArgsDetails {
  /* implicit */ NfsArgsDetails(
      std::string argStr,
      std::optional<InodeNumber> ino = std::nullopt)
      : str{std::move(argStr)}, inode{ino} {}

  std::string str;
  std::optional<InodeNumber> inode;
};

struct NfsTraceEvent : TraceEventBase {
  enum Type : unsigned char {
    START,
    FINISH,
  };

  NfsTraceEvent() = delete;

  static NfsTraceEvent start(uint32_t xid, uint32_t procNumber) {
    return NfsTraceEvent{
        xid, procNumber, StartDetails{std::unique_ptr<NfsArgsDetails>{}}};
  }

  static NfsTraceEvent
  start(uint32_t xid, uint32_t procNumber, NfsArgsDetails&& args) {
    return NfsTraceEvent{
        xid, procNumber, StartDetails{std::make_unique<NfsArgsDetails>(args)}};
  }

  static NfsTraceEvent finish(uint32_t xid, uint32_t procNumber) {
    return NfsTraceEvent{xid, procNumber, FinishDetails{}};
  }

  Type getType() const {
    return std::holds_alternative<StartDetails>(details_) ? Type::START
                                                          : Type::FINISH;
  }

  uint32_t getXid() const {
    return xid_;
  }

  uint32_t getProcNumber() const {
    return procNumber_;
  }

  // `getArguments` and `getInode` must only be called on a start event. The
  // caller is responsible to check this.
  std::optional<folly::StringPiece> getArguments() const {
    auto& argDetails = std::get<StartDetails>(details_).argDetails;
    return argDetails ? std::make_optional<folly::StringPiece>(argDetails->str)
                      : std::nullopt;
  }
  std::optional<InodeNumber> getInode() const {
    auto& argDetails = std::get<StartDetails>(details_).argDetails;
    return argDetails ? argDetails->inode : std::nullopt;
  }

 private:
  struct StartDetails {
    explicit StartDetails(std::unique_ptr<NfsArgsDetails> args)
        : argDetails{std::move(args)} {}
    std::unique_ptr<NfsArgsDetails> argDetails;
  };

  struct FinishDetails {};

  using Details = std::variant<StartDetails, FinishDetails>;

  NfsTraceEvent(uint32_t xid, uint32_t procNumber, Details&& details)
      : xid_{xid}, procNumber_{procNumber}, details_{std::move(details)} {}

  uint32_t xid_;
  uint32_t procNumber_;
  Details details_;
};

class Nfsd3 {
 public:
  /**
   * Create a new RPC NFSv3 program.
   *
   * If registerWithRpcbind is set, this NFSv3 program will advertise itself
   * against the rpcbind daemon allowing it to be visible system wide. Be aware
   * that for a given transport (tcp/udp) only one NFSv3 program can be
   * registered with rpcbind, and thus if a real NFS server is running on this
   * host, EdenFS won't be able to register itself.
   *
   * All the socket processing will be run on the EventBase passed in. This
   * also must be called on that EventBase thread.
   *
   * Note: at mount time, EdenFS will manually call mount.nfs with -o port
   * to manually specify the port on which this server is bound, so registering
   * is not necessary for a properly behaving EdenFS.
   */
  Nfsd3(
      folly::EventBase* evb,
      std::shared_ptr<folly::Executor> threadPool,
      std::unique_ptr<NfsDispatcher> dispatcher,
      const folly::Logger* straceLogger,
      std::shared_ptr<ProcessNameCache> processNameCache,
      std::shared_ptr<FsEventLogger> fsEventLogger,
      const std::shared_ptr<StructuredLogger>& structuredLogger,
      folly::Duration requestTimeout,
      std::shared_ptr<Notifier> notifications,
      CaseSensitivity caseSensitive,
      uint32_t iosize,
      size_t traceBusCapacity);

  /**
   * This is triggered when the kernel closes the socket. The socket is closed
   * when the privhelper or a user runs umount.
   */
  ~Nfsd3();

  void initialize(folly::SocketAddress addr, bool registerWithRpcbind);
  void initialize(folly::File&& connectedSocket);

  /**
   * Trigger an invalidation for the given path.
   *
   * To avoid a very large amount of traffic between an NFS client and the
   * server, the client will cache attributes that the server previously
   * returned for a file. This allows stat(2) calls to be fully resolved on the
   * client.
   *
   * NFS v3 does not support explicit invalidation. We are hacking this in.
   *
   * This invalidate method simply tries to chmod the given path in a
   * background thread.
   *
   * We rely on 2 things here:
   *   1. chmod goes all the way through the kernel to EdenFS. All "writes"
   *   seem to function this way.
   *   2. When the kernel sees the mtime in the post op attr in the response
   *   from EdenFS has updated in the response to chmod, it will drop it's
   *   caches for the children of the directory.
   *
   * 1. is implied by the NFS mode 2. isn't really guaranteed anywhere, but
   * this works well enough on Linux and macOS and we don't have many other
   * options.
   *
   * We use to just do an open call here. This was insufficient because the
   * open and subsequent reads can be served purely from cache on macOS.
   * This was sufficient on Linux as all open calls go to EdenFS and CTO
   * (close-to-open) guarantees from NFS guarantees the caches must be flushed.
   *
   * Note that the chmod(2) call runs asynchronously in a background thread as
   * both the kernel and EdenFS are holding locks that would otherwise cause
   * EdenFS to deadlock. The flushInvalidations method below should be called
   * with all the locks released to wait for all the invalidation to complete.
   */
  void invalidate(AbsolutePath path, mode_t mode);

  void takeoverStop();

  /**
   * Wait for all pending invalidation to complete.
   *
   * The future will complete when all the previously triggered invalidation
   * completed.
   */
  folly::Future<folly::Unit> flushInvalidations();

  /**
   * Obtain the address that this NFSv3 program is listening on.
   */
  folly::SocketAddress getAddr() const {
    return server_->getAddr();
  }

  struct OutstandingRequest {
    uint32_t xid;
    std::chrono::steady_clock::time_point requestStartTime;
  };

  using StopData = RpcStopData;
  /**
   * Return a future that will be triggered on unmount.
   */
  folly::SemiFuture<StopData> getStopFuture();

  ProcessAccessLog& getProcessAccessLog() {
    return processAccessLog_;
  }

  Nfsd3(const Nfsd3&) = delete;
  Nfsd3(Nfsd3&&) = delete;
  Nfsd3& operator=(const Nfsd3&) = delete;
  Nfsd3& operator=(Nfsd3&&) = delete;

  /**
   * Returns the approximate set of outstanding NFS requests. Since
   * telemetry is tracked on a background thread, the result may very slightly
   * lag reality.
   */
  std::vector<Nfsd3::OutstandingRequest> getOutstandingRequests();

  /**
   * While the returned handle is alive, NfsTraceEvents published on the
   * TraceBus will have detailed argument strings.
   */
  TraceDetailedArgumentsHandle traceDetailedArguments();

  TraceBus<NfsTraceEvent>& getTraceBus() {
    return *traceBus_;
  }

 private:
  struct TelemetryState {
    std::unordered_map<uint64_t, OutstandingRequest> requests;
  };
  folly::Synchronized<TelemetryState> telemetryState_;
  std::vector<TraceSubscriptionHandle<NfsTraceEvent>> traceSubscriptionHandles_;

  folly::Promise<StopData> stopPromise_;
  std::shared_ptr<RpcServer> server_;
  ProcessAccessLog processAccessLog_;
  // It is critical that this is a SerialExecutor. invalidation for parent
  // directories should happen after children, and we flush invalidations by
  // adding one work item to the queue.
  folly::Executor::KeepAlive<folly::Executor> invalidationExecutor_;
  std::atomic<size_t> traceDetailedArguments_;
  // The TraceBus must be the last member because its subscribed functions may
  // close over `this` and can run until the TraceBus itself is deallocated.
  std::shared_ptr<TraceBus<NfsTraceEvent>> traceBus_;
};

folly::StringPiece nfsProcName(uint32_t procNumber);
ProcessAccessLog::AccessType nfsProcAccessType(uint32_t procNumber);
} // namespace facebook::eden

#endif
