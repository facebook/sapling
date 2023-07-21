/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

// Implementation of the mount protocol as described in:
// https://tools.ietf.org/html/rfc1813#page-106

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class EventBase;
class Executor;
} // namespace folly

namespace facebook::eden {

class RpcbinddServerProcessor;
class StructuredLogger;
class RpcServer;

class Rpcbindd {
 public:
  /**
   * Create a new RPC Rpcbindd program. Rpcbind also known as PortMapper is
   * an RPC program that allows finding other RPC programs running on a
   * machine. Rpcbind runs on a known port (111). Other RPC servers running
   * on the same machine are suppose to register themselves with the rpcbind
   * server. Then clients running on other machines can query the rpcbind
   * program to learn which port the other RPC services are running on.
   *
   * All the socket processing will be run on the EventBase passed in. This
   * also must be called on that EventBase thread.
   */
  Rpcbindd(
      folly::EventBase* evb,
      std::shared_ptr<folly::Executor> threadPool,
      const std::shared_ptr<StructuredLogger>& structuredLogger);

  /**
   * Start the rpcbind service
   */
  void initialize();

  /**
   * Registers an RPC service running a certain protocol version on port.
   */
  void recordPortNumber(uint32_t protocol, uint32_t version, uint16_t port);

  Rpcbindd(const Rpcbindd&) = delete;
  Rpcbindd(Rpcbindd&&) = delete;
  Rpcbindd& operator=(const Rpcbindd&) = delete;
  Rpcbindd& operator=(Rpcbindd&&) = delete;

 private:
  std::shared_ptr<RpcbinddServerProcessor> proc_;
  std::shared_ptr<RpcServer> server_;
};

} // namespace facebook::eden
