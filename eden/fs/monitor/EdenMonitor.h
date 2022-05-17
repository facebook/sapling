/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/Range.h>
#include <folly/io/async/EventBase.h>

#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
}

namespace apache::thrift {
template <class>
class Client;
} // namespace apache::thrift

namespace facebook::eden {

class EdenConfig;
class EdenInstance;
class EdenService;
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
  explicit EdenMonitor(
      std::unique_ptr<EdenConfig> config,
      folly::StringPiece selfExe,
      const std::vector<std::string>& selfArgv);
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
  std::shared_ptr<apache::thrift::Client<EdenService>> createEdenThriftClient();

  /**
   * Request that this monitor daemon restart itself.
   */
  void performSelfRestart();

  /**
   * edenInstanceFinished() should be called by the EdenInstance object when the
   * EdenFS process that it is monitoring has exited.
   */
  void edenInstanceFinished(EdenInstance* instance);

 private:
  class SignalHandler;
  enum class State {
    Starting,
    Running,
  };

  EdenMonitor(EdenMonitor const&) = delete;
  EdenMonitor& operator=(EdenMonitor const&) = delete;

  folly::Future<folly::Unit> start();
  folly::Future<folly::Unit> getEdenInstance();

  void signalReceived(int sig);

  State state_{State::Starting};
  AbsolutePath const edenDir_;
  folly::EventBase eventBase_;
  std::unique_ptr<SignalHandler> signalHandler_;
  std::unique_ptr<EdenInstance> edenfs_;
  std::shared_ptr<LogFile> log_;

  std::string selfExe_;
  std::vector<std::string> selfArgv_;

  // If we are performing a graceful restart this contains the new EdenFS
  // process that is starting and attempting to take over state from edenfs_.
  // Otherwise this variable will be null.
  std::unique_ptr<EdenInstance> gracefulRestartNewEdenfs_;
};

} // namespace facebook::eden
