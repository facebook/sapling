/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <tuple>
#include "eden/fs/nfs/Mountd.h"
#include "eden/fs/nfs/Nfsd3.h"

namespace facebook::eden {

class Notifications;
class ProcessNameCache;

class NfsServer {
 public:
  /**
   * Create a new NFS server.
   *
   * This will handle the lifetime of the various programs involved in the NFS
   * protocol including mountd and nfsd.
   *
   * One mountd program will be created per NfsServer, while one nfsd program
   * will be created per-mount point, this allows nfsd program to be only aware
   * of its own mount point which greatly simplifies it.
   *
   * See Mountd constructor for the meaning of registerMountdWithRpcbind.
   */
  NfsServer(bool registerMountdWithRpcbind, folly::EventBase* evb)
      : evb_(evb), mountd_(registerMountdWithRpcbind, evb_) {}

  /**
   * Return value of registerMount.
   */
  struct NfsMountInfo {
    std::unique_ptr<Nfsd3> nfsd;
    uint16_t mountdPort;
    uint16_t nfsdPort;
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
      folly::Duration requestTimeout,
      Notifications* FOLLY_NULLABLE notifications,
      bool caseSensitive);

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

  NfsServer(const NfsServer&) = delete;
  NfsServer(NfsServer&&) = delete;
  NfsServer& operator=(const NfsServer&) = delete;
  NfsServer& operator=(NfsServer&&) = delete;

 private:
  folly::EventBase* evb_;
  Mountd mountd_;
};

} // namespace facebook::eden

#endif
