/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <tuple>
#include "eden/fs/nfs/Mountd.h"
#include "eden/fs/nfs/Nfsd3.h"
#include "eden/fs/utils/CaseSensitivity.h"

namespace folly {
class Executor;
}

namespace facebook::eden {

class Notifier;
class ProcessNameCache;

class NfsServer {
 public:
  /**
   * Create a new NFS server.
   *
   * This will handle the lifetime of the various programs involved in the NFS
   * protocol including mountd and nfsd. The requests will be serviced by a
   * blocking thread pool initialized with numServicingThreads and
   * maxInflightRequests.
   *
   * One mountd program will be created per NfsServer, while one nfsd program
   * will be created per-mount point, this allows nfsd program to be only aware
   * of its own mount point which greatly simplifies it.
   */
  NfsServer(
      folly::EventBase* evb,
      uint64_t numServicingThreads,
      uint64_t maxInflightRequests,
      const std::shared_ptr<StructuredLogger>& structuredLogger);

  /**
   * Bind the NfsServer to the passed in socket.
   *
   * See Mountd::initialize for the meaning of registerMountdWithRpcbind.
   */
  void initialize(folly::SocketAddress addr, bool registerMountdWithRpcbind);
  void initialize(folly::File&& socket);

  /**
   * Return value of registerMount.
   */
  struct NfsMountInfo {
    std::unique_ptr<Nfsd3> nfsd;
    folly::SocketAddress mountdAddr;
  };

  /**
   * Register a path as the root of a mount point.
   *
   * This will create an nfs program for that mount point and register it with
   * the mountd program.
   *
   * @return: the created nfsd program as well as a tuple that holds the TCP
   * port number that mountd and nfsd are listening to.
   */
  NfsServer::NfsMountInfo registerMount(
      AbsolutePathPiece path,
      InodeNumber rootIno,
      std::unique_ptr<NfsDispatcher> dispatcher,
      const folly::Logger* straceLogger,
      std::shared_ptr<ProcessNameCache> processNameCache,
      std::shared_ptr<FsEventLogger> fsEventLogger,
      const std::shared_ptr<StructuredLogger>& structuredLogger,
      folly::Duration requestTimeout,
      std::shared_ptr<Notifier> notifier,
      CaseSensitivity caseSensitive,
      uint32_t iosize,
      size_t traceBusCapacity);

  /**
   * Unregister the mount point matching the path.
   *
   * The nfs program will also be destroyed, and thus it is expected that
   * EdenFS has unmounted this mount point before calling this function.
   */
  void unregisterMount(AbsolutePathPiece path);

  /**
   * Return the EventBase that the various NFS programs are running on.
   */
  folly::EventBase* getEventBase() const {
    return evb_;
  }

  folly::SemiFuture<folly::File> takeoverStop();

  NfsServer(const NfsServer&) = delete;
  NfsServer(NfsServer&&) = delete;
  NfsServer& operator=(const NfsServer&) = delete;
  NfsServer& operator=(NfsServer&&) = delete;

 private:
  folly::EventBase* evb_;
  std::shared_ptr<folly::Executor> threadPool_;
  Mountd mountd_;
};

} // namespace facebook::eden

#endif
