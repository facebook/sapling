/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

// Implementation of the mount protocol as described in:
// https://tools.ietf.org/html/rfc1813#page-106

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/nfs/rpc/Server.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class MountdServerProcessor;

class Mountd {
 public:
  Mountd();

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

  Mountd(const Mountd&) = delete;
  Mountd(Mountd&&) = delete;
  Mountd& operator=(const Mountd&) = delete;
  Mountd& operator=(Mountd&&) = delete;

 private:
  std::shared_ptr<MountdServerProcessor> proc_;
  RpcServer server_;
};

} // namespace facebook::eden
