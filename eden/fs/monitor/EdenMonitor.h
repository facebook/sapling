/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/io/async/EventBase.h>

#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {

class EdenInstance;
class EdenServiceAsyncClient;
class LogFile;

/**
 * EdenMonitor is the main singleton that drives the monitoring process.
 *
 * In general it manages a single EdenInstance object (which tracks a single
 * edenfs daemon process).  However, EdenMonitor can also be asked to perform a
 * graceful restart, in which case it will start a new EdenInstance and
 * transition to monitoring the new EdenInstance object.
 *
 * The entire EdenMonitor is designed to be single threaded, using an EventBase
 * to manage I/O operations and timeouts on this one thread.  It does not
 * perform any synchronization/locking since all operation is done on a single
 * thread.
 */
class EdenMonitor {
 public:
  explicit EdenMonitor(AbsolutePathPiece edenDir);
  ~EdenMonitor();

  void run();

  folly::EventBase* getEventBase() {
    return &eventBase_;
  }
  const AbsolutePath& getEdenDir() const {
    return edenDir_;
  }

  /**
   * Create a EdenFS thrift client.
   *
   * This will start the connection attempt, but will return the new
   * EdenServiceAsyncClient object immediately.  The connection attempt likely
   * will still be in progress when this function returns.
   */
  std::shared_ptr<EdenServiceAsyncClient> createEdenThriftClient();

  /**
   * edenInstanceFinished() should be called by the EdenInstance object when the
   * EdenFS process that it is monitoring has exited.
   */
  void edenInstanceFinished(EdenInstance* instance);

 private:
  class SignalHandler;

  EdenMonitor(EdenMonitor const&) = delete;
  EdenMonitor& operator=(EdenMonitor const&) = delete;

  folly::Future<folly::Unit> start();
  folly::Future<folly::Unit> getEdenInstance();

  void signalReceived(int sig);

  AbsolutePath const edenDir_;
  folly::EventBase eventBase_;
  std::unique_ptr<SignalHandler> signalHandler_;
  std::unique_ptr<EdenInstance> edenfs_;
  std::shared_ptr<LogFile> log_;

  // If we are performing a graceful restart this contains the new EdenFS
  // process that is starting and attempting to take over state from edenfs_.
  // Otherwise this variable will be null.
  std::unique_ptr<EdenInstance> gracefulRestartNewEdenfs_;
};

} // namespace eden
} // namespace facebook
