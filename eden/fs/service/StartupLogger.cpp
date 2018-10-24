/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/service/StartupLogger.h"

#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <sys/types.h>

#ifdef EDEN_WIN
#include <folly/portability/Unistd.h>
#else
#include <sys/wait.h>
#include <sysexits.h>
#include <unistd.h>
#endif

using folly::checkUnixError;
using folly::File;
using folly::StringPiece;
using std::string;

namespace facebook {
namespace eden {

DEFINE_bool(
    foreground,
    false,
    "Run edenfs in the foreground, rather than daemonizing "
    "as a background process");

std::unique_ptr<StartupLogger> daemonizeIfRequested(
    folly::StringPiece logPath) {
  if (FLAGS_foreground) {
    auto startupLogger = std::make_unique<ForegroundStartupLogger>();
    return startupLogger;
  } else {
    auto startupLogger = std::make_unique<DaemonStartupLogger>();
    startupLogger->daemonize(logPath);
    return startupLogger;
  }
}

StartupLogger::~StartupLogger() = default;

void StartupLogger::success() {
  writeMessage(
      folly::LogLevel::INFO,
      folly::to<string>("Started edenfs (pid ", getpid(), ")"));
  successImpl();
}

void StartupLogger::writeMessage(folly::LogLevel level, StringPiece message) {
  static folly::Logger logger("eden.fs.startup");
  FB_LOG_RAW(logger, level, __FILE__, __LINE__, __func__) << message;
  writeMessageImpl(level, message);
}

void DaemonStartupLogger::successImpl() {
  if (!logPath_.empty()) {
    writeMessage(
        folly::LogLevel::INFO,
        folly::to<string>("Logs available at ", logPath_));
  }
  sendResult(0);
}

void DaemonStartupLogger::failAndExitImpl(uint8_t exitCode) {
  sendResult(exitCode);
  exit(exitCode);
}

void DaemonStartupLogger::writeMessageImpl(
    folly::LogLevel level,
    StringPiece message) {
  auto& file = origStderr_;
  if (file) {
    std::array<iovec, 2> iov;
    iov[0].iov_base = const_cast<char*>(message.data());
    iov[0].iov_len = message.size();
    constexpr StringPiece newline("\n");
    iov[1].iov_base = const_cast<char*>(newline.data());
    iov[1].iov_len = newline.size();

    // We intentionally don't check the return code from writevFull()
    // There is not much we can do if it fails.
    (void)folly::writevFull(file.fd(), iov.data(), iov.size());
  }
}

void DaemonStartupLogger::sendResult(ResultType result) {
  if (pipe_) {
    auto bytesWritten = folly::writeFull(pipe_.fd(), &result, sizeof(result));
    if (bytesWritten < 0) {
      XLOG(ERR) << "error writing result to startup log pipe: "
                << folly::errnoStr(errno);
    }
    pipe_.close();
  }

  // Close the original stderr file descriptors once initialization is complete.
  origStderr_.close();

  // Call setsid() to create a new process group and detach from the
  // controlling TTY (if we had one).  We do this in sendResult() rather than in
  // prepareChildProcess() so that we will still receive SIGINT if the user
  // presses Ctrl-C during initialization.
  setsid();
}

File DaemonStartupLogger::createPipe() {
  // Create the pipe for communication between the processes
  std::array<int, 2> pipeFDs;
  auto rc = pipe2(pipeFDs.data(), O_CLOEXEC);
  checkUnixError(rc, "failed to create communication pipes for daemonization");
  pipe_ = folly::File(pipeFDs[1], /*ownsFd=*/true);

  return folly::File(pipeFDs[0], /*ownsFd=*/true);
}

void DaemonStartupLogger::daemonize(StringPiece logPath) {
  auto parentInfo = daemonizeImpl(logPath);
  if (parentInfo) {
    auto pid = parentInfo->first;
    auto& readPipe = parentInfo->second;
    runParentProcess(std::move(readPipe), pid, logPath);
  }
}

std::optional<std::pair<pid_t, File>> DaemonStartupLogger::daemonizeImpl(
    StringPiece logPath) {
  DCHECK(!logPath.empty());

  auto readPipe = createPipe();
  logPath_ = logPath.str();

  fflush(stdout);
  fflush(stderr);

  // fork
  auto pid = fork();
  checkUnixError(pid, "failed to fork for daemonization");
  if (pid == 0) {
    // Child process.
    readPipe.close();
    prepareChildProcess(logPath);
    return std::nullopt;
  }

  // Parent process.
  pipe_.close();
  return std::make_pair(pid, std::move(readPipe));
}

void DaemonStartupLogger::runParentProcess(
    File readPipe,
    pid_t childPid,
    StringPiece logPath) {
  // Wait for the child to finish initializing itself and then exit
  // without ever returning to the caller.
  try {
    auto result = waitForChildStatus(readPipe, childPid, logPath);
    if (!result.errorMessage.empty()) {
      fprintf(stderr, "%s\n", result.errorMessage.c_str());
      fflush(stderr);
    }
    _exit(result.exitCode);
  } catch (const std::exception& ex) {
    // Catch exceptions to make sure we don't accidentally propagate them
    // out of daemonize() in the parent process.
    fprintf(
        stderr,
        "unexpected error in daemonization parent process: %s\n",
        folly::exceptionStr(ex).c_str());
    fflush(stderr);
    _exit(EX_SOFTWARE);
  }
}

void DaemonStartupLogger::prepareChildProcess(StringPiece logPath) {
  // Redirect stdout & stderr
  redirectOutput(logPath);
}

void DaemonStartupLogger::redirectOutput(StringPiece logPath) {
  try {
    logPath_ = logPath.str();

    // Save a copy of the original stderr descriptors, so we can still write
    // startup status messages directly to this descriptor.  This will be closed
    // once we complete initialization.
    origStderr_ = File(STDERR_FILENO, /*ownsFd=*/false).dup();

    File logHandle(logPath, O_APPEND | O_CREAT | O_WRONLY | O_CLOEXEC, 0644);
    checkUnixError(dup2(logHandle.fd(), STDOUT_FILENO));
    checkUnixError(dup2(logHandle.fd(), STDERR_FILENO));
  } catch (const std::exception& ex) {
    exitUnsuccessfully(
        EX_IOERR,
        "error opening log file ",
        logPath,
        ": ",
        folly::exceptionStr(ex));
  }
}

DaemonStartupLogger::ParentResult DaemonStartupLogger::waitForChildStatus(
    const File& pipe,
    pid_t childPid,
    StringPiece logPath) {
  ResultType status;
  auto bytesRead = folly::readFull(pipe.fd(), &status, sizeof(status));
  if (bytesRead < 0) {
    return ParentResult(
        EX_SOFTWARE,
        "error reading status of edenfs initialization: ",
        folly::errnoStr(errno));
  }

  if (static_cast<size_t>(bytesRead) < sizeof(status)) {
    // This should only happen if edenfs crashed before writing its status.
    // Check to see if the child process has died.
    auto result = handleChildCrash(childPid);
    result.errorMessage += folly::to<string>(
        "\nCheck the edenfs log file at ", logPath, " for more details");
    return result;
  }

  // Return the status code.
  // The daemon process should have already printed a message about it status.
  return ParentResult(status);
}

DaemonStartupLogger::ParentResult DaemonStartupLogger::handleChildCrash(
    pid_t childPid) {
  int status;
  auto waitedPid = waitpid(childPid, &status, WNOHANG);
  if (waitedPid == childPid) {
    if (WIFSIGNALED(status)) {
      return ParentResult(
          EX_SOFTWARE,
          "error: edenfs crashed with signal ",
          WTERMSIG(status),
          " before it finished initializing");
    } else if (WIFEXITED(status)) {
      int exitCode = WEXITSTATUS(status);
      if (exitCode == 0) {
        // We don't ever want to exit successfully in this case, even if
        // the edenfs daemon somehow did.
        exitCode = EX_SOFTWARE;
      }
      return ParentResult(
          exitCode,
          "error: edenfs exited with status ",
          WEXITSTATUS(status),
          " before it finished initializing");
    } else {
      // This is unlikely to occur; it potentially means something attached to
      // the child with ptrace.
      return ParentResult(
          EX_SOFTWARE,
          "error: edenfs stopped unexpectedly before it "
          "finished initializing");
    }
  }

  if (waitedPid == 0) {
    // The child hasn't actually exited yet.
    // This should only happen if there is a bug somehow.
    return ParentResult(
        EX_SOFTWARE,
        "error: edenfs is still running but did not report "
        "its initialization status");
  }

  string msg = "error: edenfs did not report its initialization status";
  if (waitedPid == -1) {
    // Something went wrong trying to wait.  Also report that error.
    msg += folly::to<string>(
        "\nerror: error checking status of edenfs daemon: ",
        folly::errnoStr(errno));
  }
  return ParentResult(EX_SOFTWARE, msg);
}

void ForegroundStartupLogger::writeMessageImpl(folly::LogLevel, StringPiece) {}

void ForegroundStartupLogger::successImpl() {}

[[noreturn]] void ForegroundStartupLogger::failAndExitImpl(uint8_t exitCode) {
  exit(exitCode);
}

} // namespace eden
} // namespace facebook
