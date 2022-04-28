/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/monitor/EdenMonitor.h"

#include <folly/ExceptionString.h>
#include <folly/SocketAddress.h>
#include <folly/futures/Future.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <folly/io/async/AsyncSocket.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/monitor/EdenInstance.h"
#include "eden/fs/monitor/LogFile.h"
#include "eden/fs/monitor/LogRotation.h"
#include "eden/fs/service/gen-cpp2/EdenServiceAsyncClient.h"

#ifdef __linux__
#include <systemd/sd-daemon.h> // @manual
#endif

using apache::thrift::HeaderClientChannel;
using folly::AsyncSocket;
using folly::Future;
using folly::SocketAddress;
using folly::Try;
using folly::Unit;
using std::make_unique;
using std::string;
using std::unique_ptr;

DEFINE_bool(
    restart,
    false,
    "Indicate that an in-place restart of the monitor is being performed");
DEFINE_int64(
    childEdenFSPid,
    -1,
    "The process ID of an existing EdenFS child process "
    "(only valid with --restart)");
DEFINE_int64(
    childEdenFSPipe,
    -1,
    "The log pipe FD connected to an existing EdenFS child process "
    "(only valid with --restart)");

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

EdenMonitor::EdenMonitor(
    unique_ptr<EdenConfig> config,
    folly::StringPiece selfExe,
    const std::vector<std::string>& selfArgv)
    : edenDir_{config->edenDir.getValue()},
      selfExe_(selfExe),
      selfArgv_(selfArgv) {
  signalHandler_ = make_unique<SignalHandler>(this);
  signalHandler_->registerSignalHandler(SIGCHLD);
  signalHandler_->registerSignalHandler(SIGHUP);
  signalHandler_->registerSignalHandler(SIGINT);
  signalHandler_->registerSignalHandler(SIGTERM);
  // Eventually we should register some other signals for additional actions.
  // Perhaps:
  // - SIGUSR1: request a graceful restart when the system looks idle
  // - SIGUSR2: request a hard restart (exit) when the system looks idle

  auto logDir = edenDir_ + "logs"_relpath;
  ensureDirectoryExists(logDir);

  auto maxLogSize = config->maxLogFileSize.getValue();
  unique_ptr<LogRotationStrategy> rotationStrategy;
  if (maxLogSize > 0) {
    rotationStrategy = make_unique<TimestampLogRotation>(
        config->maxRotatedLogFiles.getValue());
  }
  log_ = std::make_shared<LogFile>(
      logDir + "edenfs.log"_relpath, maxLogSize, std::move(rotationStrategy));
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
    state_ = State::Running;
#ifdef __linux__
    auto rc = sd_notify(/*unset_environment=*/false, "READY=1");
    if (rc < 0) {
      XLOG(ERR) << "sd_notify READY=1 failed: " << folly::errnoStr(-rc);
    }
#endif
  });
}

Future<Unit> EdenMonitor::getEdenInstance() {
  // If --restart was specified and we are restarting with an existing child
  // EdenFS process, create a SpawnedEdenInstance object to take it over.
  if (FLAGS_restart && FLAGS_childEdenFSPid) {
    XLOG(INFO) << "taking over management of existing EdenFS daemon "
               << FLAGS_childEdenFSPid;
    auto edenfs = std::make_unique<SpawnedEdenInstance>(this, log_);
    edenfs->takeover(FLAGS_childEdenFSPid, FLAGS_childEdenFSPipe);
    edenfs_ = std::move(edenfs);
    return folly::makeFuture();
  }

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
  auto socket = AsyncSocket::newSocket(
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

void EdenMonitor::performSelfRestart() {
  // For now, ignore SIGHUP requests while EdenFS is still starting.
  // While we could have the new EdenFS daemon be aware that it still needs to
  // wait for the EdenFS process to start, the simplest behavior for now is to
  // not allow self-restarts during this time.  Being able to perform a
  // self-restart while EdenFS is restarting is not terribly important.
  if (state_ == State::Starting) {
    XLOG(WARN)
        << "ignoring self-restart request for the EdenFS monitor: "
        << "EdenFS is still starting.  Attempt this again once EdenFS has started.";
    return;
  }

  // Build a vector of extra arguments to pass along with information about
  // the EdenFS process we are currently monitoring.
  std::vector<string> extraRestartArgs;
  int childPipeFd = -1;
  auto spawnedEdenfs = dynamic_cast<SpawnedEdenInstance*>(edenfs_.get());
  if (spawnedEdenfs) {
    childPipeFd = spawnedEdenfs->getLogPipeFD();
    extraRestartArgs.push_back("--childEdenFSPid");
    extraRestartArgs.push_back(folly::to<string>(edenfs_->getPid()));
    extraRestartArgs.push_back("--childEdenFSPipe");
    extraRestartArgs.push_back(folly::to<string>(childPipeFd));
  }

  // Prepare the vector of raw const char* pointers to pass to execv()
  std::vector<const char*> argv;
  for (const auto& arg : selfArgv_) {
    // The --restart flag indicates the start of arguments that are specific
    // to this restart invocation.  Do not forward any arguments after this,
    // so we don't pass old --childEdenFSPid and --childEdenFSPipe flags
    // from the previous time we were restarted.
    if (arg == "--restart") {
      break;
    }
    argv.push_back(arg.c_str());
  }
  argv.push_back("--restart");
  for (const auto& arg : extraRestartArgs) {
    argv.push_back(arg.c_str());
  }
  argv.push_back(nullptr);

  // Clear the O_CLOEXEC flag on the child pipe
  if (childPipeFd != -1) {
    int rc = fcntl(childPipeFd, F_SETFD, 0);
    folly::checkUnixError(rc, "failed to clear CLOEXEC flag on child log pipe");
  }

  XLOG(INFO) << "Restarting EdenFS monitor in place...";
  XLOG(DBG2) << "Restart exe: " << selfExe_;
  XLOG(DBG2) << "Restart args: "
             << folly::join(" ", argv.begin(), argv.end() - 1);
  execv(selfExe_.c_str(), const_cast<char**>(argv.data()));

  XLOG(ERR) << "failed to perform self-restart: " << folly::errnoStr(errno);
  // Restore the O_CLOEXEC flag on the child pipe
  if (childPipeFd != -1) {
    int rc = fcntl(childPipeFd, F_SETFD, FD_CLOEXEC);
    if (rc != 0) {
      XLOG(ERR) << "failed to restore CLOEXEC flag on log pipe: "
                << folly::errnoStr(errno);
    }
  }
}

void EdenMonitor::signalReceived(int sig) {
  switch (sig) {
    case SIGCHLD:
      XLOG(DBG2) << "got SIGCHLD";
      edenfs_->checkLiveness();
      return;
    case SIGHUP:
      performSelfRestart();
      return;
    case SIGINT:
    case SIGTERM:
      // Forward the signal to the edenfs instance
      XLOG(DBG1) << "received terminal signal " << sig;
      auto pid = edenfs_->getPid();
      XCHECK_GE(pid, 0);
      auto rc = kill(pid, sig);
      if (rc != 0) {
        XLOG(WARN) << "error forwarding signal " << sig
                   << " to EdenFS: " << folly::errnoStr(errno);
      }
      return;
  }
  XLOG(WARN) << "received unexpected signal " << sig;
}

} // namespace eden
} // namespace facebook
