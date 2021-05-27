/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/ProcessAccessLog.h"

namespace folly {
class Executor;
}

namespace facebook::eden {

class Notifications;
class ProcessNameCache;

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
      folly::Duration requestTimeout,
      Notifications* FOLLY_NULLABLE notifications,
      CaseSensitivity caseSensitive,
      uint32_t iosize);

  /**
   * This is triggered when the kernel closes the socket. The socket is closed
   * when the privhelper or a user runs umount.
   */
  ~Nfsd3();

  void initialize(folly::SocketAddress addr, bool registerWithRpcbind);

  /**
   * Trigger an invalidation for the given path.
   *
   * To avoid a very large amount of traffic between an NFS client and the
   * server, the client will cache attributes that the server previously
   * returned for a file. This allows stat(2) calls to be fully resolved on the
   * client. However, clients do respect a close-to-open consistency (CTO)
   * whereas opening a file will refresh the client attributes. This invalidate
   * method simply tries to open the given file in a background thread.
   *
   * Note that the open(2) call runs asynchronously in a background thread as
   * both the kernel and EdenFS are holding locks that would otherwise cause
   * EdenFS to deadlock. The flushInvalidations method below should be called
   * with all the locks released to wait for all the invalidation to complete.
   */
  void invalidate(AbsolutePath path);

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
    return server_.getAddr();
  }

  struct StopData {};

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

 private:
  folly::Promise<StopData> stopPromise_;
  RpcServer server_;
  ProcessAccessLog processAccessLog_;
  folly::Executor::KeepAlive<folly::Executor> invalidationExecutor_;
};

} // namespace facebook::eden

#endif
