/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

// Implementation of the mount protocol as described in:
// https://tools.ietf.org/html/rfc1813#page-106

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/nfs/rpc/Server.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class Executor;
}

namespace facebook::eden {

class MountdServerProcessor;

class Mountd {
 public:
  /**
   * Create a new RPC mountd program.
   *
   * All the socket processing will be run on the EventBase passed in. This
   * also must be called on that EventBase thread.
   *
   * Note: at mount time, EdenFS will manually call mount.nfs with -o mountport
   * to manually specify the port on which this server is bound, so registering
   * is not necessary for a properly behaving EdenFS.
   */
  Mountd(folly::EventBase* evb, std::shared_ptr<folly::Executor> threadPool);

  /**
   * Bind the RPC mountd program to the passed in address.
   *
   * If registerWithRpcbind is set, this mountd program will advertise itself
   * against the rpcbind daemon allowing it to be visible system wide. Be aware
   * that for a given transport (tcp/udp) only one mountd program can be
   * registered with rpcbind, and thus if a real NFS server is running on this
   * host, EdenFS won't be able to register itself.
   */
  void initialize(folly::SocketAddress addr, bool registerWithRpcbind);
  void initialize(folly::File&& socket);

  /**
   * Register a path as the root of a mount point.
   *
   * Once registered, the mount RPC request for that specific path will answer
   * positively with the passed in InodeNumber.
   */
  void registerMount(AbsolutePathPiece path, InodeNumber rootIno);

  /**
   * Unregister the mount point matching the path.
   */
  void unregisterMount(AbsolutePathPiece path);

  /**
   * Obtain the address that this mountd program is listening on.
   */
  folly::SocketAddress getAddr() const {
    return server_->getAddr();
  }

  folly::SemiFuture<folly::File> takeoverStop();

  Mountd(const Mountd&) = delete;
  Mountd(Mountd&&) = delete;
  Mountd& operator=(const Mountd&) = delete;
  Mountd& operator=(Mountd&&) = delete;

 private:
  std::shared_ptr<MountdServerProcessor> proc_;
  std::shared_ptr<RpcServer> server_;
};

} // namespace facebook::eden

#endif
