/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/monitor/EdenMonitor.h"

#include <folly/ExceptionString.h>
#include <folly/SocketAddress.h>
#include <folly/futures/Future.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp/async/TAsyncSocket.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>

#include "eden/fs/eden-config.h"
#include "eden/fs/monitor/EdenInstance.h"
#include "eden/fs/monitor/LogFile.h"
#include "eden/fs/service/gen-cpp2/EdenServiceAsyncClient.h"

#if EDEN_HAVE_SYSTEMD
#include <systemd/sd-daemon.h> // @manual
#endif

using apache::thrift::HeaderClientChannel;
using apache::thrift::async::TAsyncSocket;
using folly::Future;
using folly::SocketAddress;
using folly::Try;
using folly::Unit;

namespace facebook {
namespace eden {

class EdenMonitor::SignalHandler : public folly::AsyncSignalHandler {
 public:
  explicit SignalHandler(EdenMonitor* monitor)
      : AsyncSignalHandler{monitor->getEventBase()}, monitor_{monitor} {}

  void signalReceived(int sig) noexcept override {
    XLOG(DBG1) << "received signal " << sig;
    try {
      monitor_->signalReceived(sig);
    } catch (const std::exception& ex) {
      XLOG(ERR) << "unexpected error handling signal " << sig << ": "
                << folly::exceptionStr(ex);
    }
  }

 private:
  EdenMonitor* const monitor_;
  folly::Promise<Unit> runningPromise_;
};

EdenMonitor::EdenMonitor(AbsolutePathPiece edenDir) : edenDir_{edenDir} {
  signalHandler_ = std::make_unique<SignalHandler>(this);
  signalHandler_->registerSignalHandler(SIGCHLD);
  signalHandler_->registerSignalHandler(SIGINT);
  signalHandler_->registerSignalHandler(SIGTERM);
  // Eventually we should register some other signals for additional actions.
  // Perhaps:
  // - SIGHUP: request that this process re-exec itself to facilitate upgrading
  //           itself
  // - SIGUSR1: request a graceful restart when the system looks idle
  // - SIGUSR2: request a hard restart (exit) when the system looks idle

  auto logDir = edenDir + "logs"_relpath;
  ensureDirectoryExists(logDir);
  log_ = std::make_shared<LogFile>(logDir + "edenfs.log"_relpath);
}

EdenMonitor::~EdenMonitor() {}

void EdenMonitor::run() {
  // Schedule our start operation to run once we start the EventBase loop
  eventBase_.runInLoop([this] {
    start().thenError([this](auto&& error) {
      XLOG(ERR) << "error starting EdenMonitor: " << error.what();
      eventBase_.terminateLoopSoon();
    });
  });

  // Run the EventBase loop
  eventBase_.loopForever();
}

Future<Unit> EdenMonitor::start() {
  return getEdenInstance().thenValue([this](auto&&) {
    XCHECK(edenfs_ != nullptr);
#if EDEN_HAVE_SYSTEMD
    auto rc = sd_notify(/*unset_environment=*/false, "READY=1");
    if (rc < 0) {
      XLOG(ERR) << "sd_notify READY=1 failed: " << folly::errnoStr(-rc);
    }
#endif
  });
}

Future<Unit> EdenMonitor::getEdenInstance() {
  // Check to see if there is an existing EdenFS already running.
  //
  // This behavior exists primarily to help gracefully enable the monitor on
  // systems that were already running EdenFS without the monitor.  We could
  // eventually remove this functionality once the monitor is widely deployed
  // and there are no remaining instances that are not using it.
  auto client = createEdenThriftClient();
  return client->future_getPid().thenTry([client, this](Try<int64_t> pid) {
    auto future = Future<Unit>::makeEmpty();
    if (pid.hasValue()) {
      XLOG(INFO) << "found existing EdenFS process " << pid.value();
      edenfs_ = std::make_unique<ExistingEdenInstance>(this, pid.value());
      future = edenfs_->start();
    } else {
      edenfs_ = std::make_unique<SpawnedEdenInstance>(this, log_);
      future = edenfs_->start();
      XLOG(INFO) << "starting new EdenFS process " << edenfs_->getPid();
    }
    return future;
  });
}

std::shared_ptr<EdenServiceAsyncClient> EdenMonitor::createEdenThriftClient() {
  auto socketPath = edenDir_ + PathComponentPiece("socket");
  uint32_t connectTimeoutMS = 500;
  auto socket = TAsyncSocket::newSocket(
      &eventBase_,
      SocketAddress::makeFromPath(socketPath.value()),
      connectTimeoutMS);
  auto channel = HeaderClientChannel::newChannel(std::move(socket));
  return std::make_shared<EdenServiceAsyncClient>(std::move(channel));
}

void EdenMonitor::edenInstanceFinished(EdenInstance* /*instance*/) {
  XLOG(DBG1) << "EdenFS has exited; terminating the monitor";
  eventBase_.terminateLoopSoon();
}

void EdenMonitor::signalReceived(int sig) {
  if (sig == SIGCHLD) {
    XLOG(DBG2) << "got SIGCHLD";
    edenfs_->checkLiveness();
    return;
  }

  // Forward the signal to the edenfs instance
  XLOG(DBG1) << "received terminal signal " << sig;
  auto pid = edenfs_->getPid();
  XCHECK_GE(pid, 0);
  auto rc = kill(pid, sig);
  if (rc != 0) {
    XLOG(WARN) << "error forwarding signal " << sig
               << " to EdenFS: " << folly::errnoStr(errno);
  }
}

} // namespace eden
} // namespace facebook
