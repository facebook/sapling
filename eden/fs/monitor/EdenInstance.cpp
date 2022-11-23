/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/monitor/EdenInstance.h"

#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>

#include "eden/fs/monitor/EdenMonitor.h"
#include "eden/fs/monitor/LogFile.h"
#include "eden/fs/service/EdenInit.h"
#include "eden/fs/service/gen-cpp2/EdenServiceAsyncClient.h"
#include "eden/fs/utils/SpawnedProcess.h"

using facebook::fb303::cpp2::fb303_status;
using folly::Future;
using folly::Try;
using folly::Unit;
using namespace std::chrono_literals;

DEFINE_string(
    edenfs,
    "/usr/local/libexec/eden/edenfs",
    "The path to the edenfs executable");
DEFINE_string(edenfsctl, "", "The path to the edenfsctl executable");
DEFINE_string(
    cat_exe,
    "/bin/cat",
    "The path to the cat executable (used for background log "
    "forwarding in some situations)");
DEFINE_int64(
    edenfs_poll_interval_ms,
    5000,
    "How frequently to poll for process liveness when monitoring an existing "
    "EdenFS daemon that we did not start");

namespace facebook::eden {

EdenInstance::EdenInstance(EdenMonitor* monitor) : monitor_{monitor} {}

EdenInstance::~EdenInstance() {}

ExistingEdenInstance::ExistingEdenInstance(EdenMonitor* monitor, pid_t pid)
    : EdenInstance(monitor),
      AsyncTimeout(monitor->getEventBase()),
      pid_(pid),
      pollInterval_{std::chrono::milliseconds(FLAGS_edenfs_poll_interval_ms)} {}

Future<Unit> ExistingEdenInstance::start() {
  scheduleTimeout(pollInterval_);
  return folly::makeFuture();
}

void ExistingEdenInstance::timeoutExpired() noexcept {
  if (isAlive()) {
    scheduleTimeout(pollInterval_);
  } else {
    monitor_->edenInstanceFinished(this);
  }
}

void ExistingEdenInstance::checkLiveness() {
  // checkLiveness() is mainly called when we receive SIGCHLD.
  // Since this edenfs process was not started by us we won't get SIGCHLD when
  // it dies.  However it doesn't hurt to go ahead and check if it has exited
  // here anyway.
  if (!isAlive()) {
    monitor_->edenInstanceFinished(this);
  }
}

bool ExistingEdenInstance::isAlive() {
  int rc = kill(pid_, 0);
  if (rc < 0 && errno == ESRCH) {
    // The process no longer exists.
    return false;
  }
  return true;
}

class SpawnedEdenInstance::StartupStatusChecker : private AsyncTimeout {
 public:
  explicit StartupStatusChecker(SpawnedEdenInstance* instance)
      : AsyncTimeout(instance->monitor_->getEventBase()), instance_{instance} {}
  ~StartupStatusChecker() override {
    if (instance_) {
      startupAborted();
    }
  }

  Future<Unit> start() {
    scheduleTimeout(pollInterval_);
    return promise_.getFuture();
  }
  void startupAborted() {
    instance_ = nullptr;
    client_.reset();
    promise_.setException(std::runtime_error("start attempt aborted"));
  }

 private:
  void timeoutExpired() noexcept override {
    folly::makeFutureWith([this]() {
      return checkRunning();
    }).thenTry([this](Try<bool> result) {
      client_.reset();
      if (result.hasValue() && result.value()) {
        edenRunning();
      } else {
        reschedule();
      }
    });
  }

  void edenRunning() {
    if (!instance_) {
      return;
    }
    instance_ = nullptr;
    promise_.setValue();
  }

  void reschedule() {
    if (!instance_) {
      return;
    }
    scheduleTimeout(pollInterval_);
  }

  Future<bool> checkRunning() {
    // Save client_ as a member variable so that we can destroy it in
    // startupAborted() to cancel the pending thrift call.
    client_ = instance_->monitor_->createEdenThriftClient();
    return client_->future_getStatus().thenTry([](Try<fb303_status> status) {
      return status.hasValue() && (status.value() == fb303_status::ALIVE);
    });
  }

  folly::Promise<Unit> promise_;
  std::chrono::milliseconds pollInterval_{200};
  SpawnedEdenInstance* instance_{nullptr};
  std::shared_ptr<EdenServiceAsyncClient> client_;
};

SpawnedEdenInstance::SpawnedEdenInstance(
    EdenMonitor* monitor,
    std::shared_ptr<LogFile> log)
    : EdenInstance(monitor),
      EventHandler(monitor->getEventBase()),
      AsyncTimeout(monitor->getEventBase()),
      edenfsExe_(canonicalPath(FLAGS_edenfs)),
      log_(std::move(log)) {}

SpawnedEdenInstance::~SpawnedEdenInstance() {
  // If we are still waiting on the StartupStatusChecker, explicitly
  // startupAborted() it when we are being destroyed.  Aborting/destroying it
  // will automatically trigger its pending promise to fail with an error.
  // Letting this happen automatically inside the StartupStatusChecker
  // destructor is a bit fragile with regards to destruction ordering, so
  // explicitly abort it now before any of our member variables are destroyed.
  if (startupChecker_) {
    startupChecker_->startupAborted();
    // We automatically reset startupChecker_ to null when its promise
    // completes.  Check that this has happened.
    XCHECK(!startupChecker_);
  }
}

Future<Unit> SpawnedEdenInstance::start() {
  auto startupLog = monitor_->getEdenDir() + "logs/startup.log"_relpath;
  // TODO: rotate/truncate the startup log
  std::vector<std::string> argv = {
      "edenfs",
      "--edenfs",
      "--foreground",
      "--edenDir",
      monitor_->getEdenDir().value(),
      "--startupLogPath",
      startupLog.value(),
  };
  if (!FLAGS_edenfsctl.empty()) {
    argv.push_back("--edenfsctlPath");
    argv.push_back(FLAGS_edenfsctl);
  }
  if (!FLAGS_etcEdenDir.empty()) {
    argv.push_back("--etcEdenDir");
    argv.push_back(FLAGS_etcEdenDir);
  }
  if (!FLAGS_etcEdenDir.empty()) {
    argv.push_back("--configPath");
    argv.push_back(FLAGS_configPath);
  }
  SpawnedProcess::Options options;
  Pipe outputPipe;
  options.dup2(outputPipe.write.duplicate(), STDOUT_FILENO);
  options.dup2(std::move(outputPipe.write), STDERR_FILENO);
  options.executablePath(edenfsExe_);

  // Execute edenfs.
  // Note that this will block until the fork() and execve() completes.
  // In practice this normally should not block for too long, so I'm not too
  // concerned about this at the moment.
  cmd_ = SpawnedProcess(argv, std::move(options));
  // Save the process pid as a member variable.  Subprocess.pid() will return -1
  // after the process has died, but we still want to be able to log the old pid
  // correctly even after the process has exited.
  pid_ = cmd_.pid();

  logPipe_ = std::move(outputPipe.read);

  beginProcessingLogPipe();

  // Wait for EdenFS to become healthy.
  //
  // Currently we do this by periodically polling with getStatus() calls.
  // Eventually it might be nicer to do this by having EdenFS write the startup
  // log messages to a pipe, and we could use the pipe closing to tell when
  // startup has finished.  For now just polling getStatus() is simplest.
  //
  // We store startupChecker_ as a member variable so that it will be destroyed
  // (and the checking cancelled) if we are destroyed.
  startupChecker_ = std::make_unique<StartupStatusChecker>(this);
  return startupChecker_->start().thenTry([this](Try<Unit> result) {
    XLOG(INFO) << "EdenFS pid " << getPid() << " has finished starting";
    startupChecker_.reset();
    return result;
  });
}

void SpawnedEdenInstance::takeover(pid_t pid, int logFD) {
  cmd_ = SpawnedProcess::fromExistingProcess(pid);
  pid_ = pid;

  logPipe_ = FileDescriptor(logFD, "takeover", FileDescriptor::FDType::Generic);
  auto rc = fcntl(logPipe_.fd(), F_SETFD, FD_CLOEXEC);
  if (rc != 0) {
    XLOG(ERR) << "failed to restore CLOEXEC flag on log pipe during restart: "
              << folly::errnoStr(errno);
  }
  beginProcessingLogPipe();
}

void SpawnedEdenInstance::handlerReady(uint16_t events) noexcept {
  XLOGF(DBG4, "handlerReady(events={:#x})", events);
  try {
    forwardLogOutput();
  } catch (const std::exception& ex) {
    XLOG(ERR) << "unexpected error forwarding EdenFS log output: "
              << folly::exceptionStr(ex);
    closeLogPipe();
  }
}

void SpawnedEdenInstance::timeoutExpired() noexcept {
  // timeoutExpired() is called when EdenFS has exited but the output pipe
  // remains open for several more seconds.
  //
  // We want to go ahead and inform monitor_ that EdenFS has exited in this
  // case, but continue forwarding output from the pipe in the background.
  // We explicitly fork a separate process to forward the output in this case.
  // Doing this in a completely separate process allows the output to still be
  // forwarded even if we exit at some point in the future.  e.g., if EdenFS
  // exits we probably want to exit ourselves too, to let systemd know that the
  // process has died.
  //
  // While we could fork() and continue forwarding the output ourselves (without
  // calling exec()), using exec() gives us a cleaner separation, and ensures
  // that any O_CLOEXEC file descriptors get closed.
  //
  // Note that forwarding with cat like this will continue writing to the old
  // log file even if the log gets rotated, but this probably shouldn't be a
  // major problem in practice.
  SpawnedProcess::Options options;
  options.dup2(
      FileDescriptor(::dup(log_->fd()), "dup", FileDescriptor::FDType::Generic),
      STDOUT_FILENO);
  options.dup2(
      FileDescriptor(::dup(log_->fd()), "dup", FileDescriptor::FDType::Generic),
      STDERR_FILENO);
  options.dup2(logPipe_.duplicate(), STDIN_FILENO);
  options.executablePath(canonicalPath(FLAGS_cat_exe));
  std::vector<std::string> argv = {"cat"};
  try {
    SpawnedProcess(argv, std::move(options)).detach();
  } catch (const std::exception& ex) {
    // Log an error.  There isn't a whole lot else we can do in this case.
    XLOG(ERR) << "failed to spawn " << FLAGS_cat_exe
              << " for forwarding logs from exited EdenFS process: "
              << folly::exceptionStr(ex);
  }

  monitor_->edenInstanceFinished(this);
}

void SpawnedEdenInstance::beginProcessingLogPipe() {
  // Start reading from Eden's stdout, and forwarding it to our log file
  auto rc = ::fcntl(logPipe_.fd(), F_SETFL, O_NONBLOCK);
  folly::checkUnixError(rc, "failed to make edenfs output pipe non-blocking");
  changeHandlerFD(folly::NetworkSocket(logPipe_.fd()));
  registerHandler(EventHandler::READ | EventHandler::PERSIST);
}

void SpawnedEdenInstance::forwardLogOutput() {
  // It would be nice if we could use splice() to forward data from the pipe to
  // the log file without copying it through userspace.  Unfortunately splice()
  // does not support writing to files in O_APPEND mode.  Using O_APPEND for the
  // log seems important just in case multiple separate processes do end up
  // writing to the log file at the same time.
  auto result = logPipe_.readNoInt(logBuffer_.data(), logBuffer_.size());
  if (result.hasException()) {
    XLOG(ERR) << "error reading EdenFS output: "
              << folly::exceptionStr(result.exception());
    if (auto exc = result.tryGetExceptionObject<std::system_error>()) {
      if (exc->code() == std::error_code(EAGAIN, std::generic_category())) {
        // This isn't really expected, since we were told that the fd was ready.
        // Just return without closing the log pipe in this case.
        return;
      }
    }
    closeLogPipe();
    return;
  }

  auto bytesRead = result.value();
  if (bytesRead == 0) {
    XLOG(DBG1) << "EdenFS output closed";
    closeLogPipe();
    return;
  }

  auto errnum = log_->write(logBuffer_.data(), bytesRead);
  if (errnum == 0) {
    XLOG(DBG3) << "forwarded " << bytesRead << " log bytes";
  } else {
    // On a write error we generally still want to keep reading from EdenFS's
    // output and attempting to write to the log file.
    //
    // e.g., if the disk fills up we will get ENOSPC errors while writing logs,
    // but we still want to keep reading from EdenFS even if we can't write the
    // log output.  EdenFS will eventually start dropping logs itself if we do
    // not read them fast enough, but other subprocess that EdenFS spawns, like
    // hg, may not behave well if we don't consume their stdout/stderr output
    // quickly.
    //
    // Only try to log about this error every minute, so we don't end up trying
    // to log a lot of messages ourself when the disk is full.
    XLOG_EVERY_MS(ERR, 60s)
        << "error writing EdenFS log output: " << folly::errnoStr(errnum);
  }
}

void SpawnedEdenInstance::closeLogPipe() {
  unregisterHandler();
  logPipe_.close();

  // If we had already noticed that EdenFS exited we can immediately inform
  // monitor_ that we have finished.
  if (cmd_.terminated()) {
    monitor_->edenInstanceFinished(this);
    return;
  }

  // We haven't noticed that EdenFS has exited yet.
  // Call checkLivenessImpl() to poll the status and take the appropriate action
  // if it has exited.
  checkLivenessImpl();
}

void SpawnedEdenInstance::checkLiveness() {
  // If we've already previously noticed that EdenFS has died then we don't need
  // to do anything else now.
  if (cmd_.terminated()) {
    return;
  }

  checkLivenessImpl();
}

void SpawnedEdenInstance::checkLivenessImpl() {
  if (!cmd_.terminated()) {
    return;
  }
  auto returnCode = cmd_.wait();

  XLOG(INFO) << "EdenFS process " << getPid() << " exited " << returnCode.str();

  // If the log pipe has been closed then we are done, and can notify monitor_
  // that EdenFS is exited.
  if (!logPipe_) {
    monitor_->edenInstanceFinished(this);
    return;
  }

  // If the log pipe is still open, then wait a few more seconds to see if gets
  // closed soon.  If it does not get closed within this timeout then we'll fork
  // a background process to continue forwarding any output to the log file
  // (e.g., maybe a child process that EdenFS spawned still has the output file
  // open), but then we'll notify the EdenMonitor of Eden's exit anyway.
  scheduleTimeout(3s);
}

} // namespace facebook::eden
