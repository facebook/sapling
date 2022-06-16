/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/service/EdenInit.h"
#include "eden/fs/telemetry/SessionId.h"

#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>
#include <folly/portability/Unistd.h>
#include <sys/types.h>
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SpawnedProcess.h"

#ifndef _WIN32
#include <sys/wait.h>
#include <sysexits.h>
#endif

#include "eden/fs/eden-config.h"

#ifdef _WIN32
#define EX_SOFTWARE 70
#define EX_IOERR 74
#endif

using folly::checkUnixError;
using folly::File;
using folly::StringPiece;
using std::string;
using namespace std::chrono_literals;

namespace facebook::eden {

DEFINE_string(
    startupLogPath,
    "",
    "If set, log messages to this file until startup completes.");

DEFINE_int32(startupLoggerFd, -1, "The control pipe for startup logging");

namespace {
void writeMessageToFile(folly::File&, folly::StringPiece);
} // namespace

std::shared_ptr<StartupLogger> daemonizeIfRequested(
    folly::StringPiece logPath,
    PrivHelper* privHelper,
    const std::vector<std::string>& argv) {
  if (!FLAGS_foreground && FLAGS_startupLoggerFd == -1) {
    auto startupLogger = std::make_shared<DaemonStartupLogger>();
    if (!FLAGS_startupLogPath.empty()) {
      startupLogger->warn(
          "Ignoring --startupLogPath because --foreground was not specified");
    }
    startupLogger->spawn(logPath, privHelper, argv);
    /* NOTREACHED */
  }
  if (FLAGS_startupLoggerFd != -1) {
    // We're the child spawned by DaemonStartupLogger::spawn above
    auto startupLogger = std::make_shared<DaemonStartupLogger>();
    startupLogger->initClient(
        logPath,
        FileDescriptor(FLAGS_startupLoggerFd, FileDescriptor::FDType::Pipe));
    return startupLogger;
  }

  if (!FLAGS_startupLogPath.empty()) {
    return std::make_shared<FileStartupLogger>(FLAGS_startupLogPath);
  }
  return std::make_shared<ForegroundStartupLogger>();
}

StartupLogger::~StartupLogger() = default;

void StartupLogger::success(uint64_t startTimeInSeconds) {
  writeMessage(
      folly::LogLevel::INFO,
      folly::to<string>(
          "Started EdenFS (pid ",
          getpid(),
          ", session_id ",
          getSessionId(),
          ") in ",
          startTimeInSeconds,
          "s"));

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
    folly::LogLevel /*level*/,
    StringPiece message) {
  auto& file = origStderr_;
  if (file) {
    writeMessageToFile(file, message);
  }
}

void DaemonStartupLogger::sendResult(ResultType result) {
  // Close the original stderr file descriptors once initialization is complete.
  origStderr_.close();

  if (pipe_) {
    auto try_ = pipe_.writeFull(&result, sizeof(result));
    if (try_.hasException()) {
      XLOG(ERR) << "error writing result to startup log pipe: "
                << folly::exceptionStr(try_.exception());
    }
    pipe_.close();
  }

#ifndef _WIN32
  // Call setsid() to create a new process group and detach from the
  // controlling TTY (if we had one).  We do this in sendResult() rather than in
  // prepareChildProcess() so that we will still receive SIGINT if the user
  // presses Ctrl-C during initialization.
  setsid();
#endif
}

void DaemonStartupLogger::spawn(
    StringPiece logPath,
    PrivHelper* privHelper,
    const std::vector<std::string>& argv) {
  auto child = spawnImpl(logPath, privHelper, argv);
  runParentProcess(std::move(child), logPath);
}

DaemonStartupLogger::ChildHandler::ChildHandler(
    SpawnedProcess&& proc,
    FileDescriptor pipe)
    : process{std::move(proc)}, exitStatusPipe{std::move(pipe)} {
#ifdef _WIN32
  stderrBridge_ = std::thread([this]() {
    auto fd = process.stderrFd();
    auto stderrHandle = GetStdHandle(STD_ERROR_HANDLE);

    constexpr size_t size = 256;
    char buffer[size];

    while (true) {
      auto read = fd.readNoInt(&buffer, size);

      // Read will end when the other end of the pipe is closed.
      if (read.hasException()) {
        break;
      }

      DWORD written = 0;
      WriteFile(stderrHandle, buffer, *read, &written, nullptr);
    }
  });
#endif
}

DaemonStartupLogger::ChildHandler::~ChildHandler() {
  if (stderrBridge_.joinable()) {
    stderrBridge_.join();
  }
}

DaemonStartupLogger::ChildHandler DaemonStartupLogger::spawnImpl(
    StringPiece logPath,
    FOLLY_MAYBE_UNUSED PrivHelper* privHelper,
    const std::vector<std::string>& argv) {
  XDCHECK(!logPath.empty());

  auto exePath = executablePath();
  auto canonPath = realpath(exePath.c_str());
  if (exePath != canonPath) {
    throw std::runtime_error(folly::to<std::string>(
        "Refusing to start because my exePath ",
        exePath,
        " is not the realpath to myself (which is ",
        canonPath,
        "). This is an unsafe installation and may be an indication of a "
        "symlink attack or similar attempt to escalate privileges"));
  }

  SpawnedProcess::Options opts;
  opts.executablePath(exePath);
  opts.nullStdin();

#ifdef _WIN32
  // Redirect to a pipe. See `StartupLogger::ChildHandler` for detail.
  opts.pipeStderr();
  // Setting `CREATE_NO_WINDOW` will make sure the daemon process is detached
  // from user's interactive console.
  opts.creationFlags(CREATE_NO_WINDOW);
#endif

  // We want to append arguments to the argv list, but we need to take
  // care for the case where the args look like:
  // ["some", "args", "--", "extra", "args"]
  // In that case we want to insert before the the "--" in order to
  // preserve the semantic meaning of the command line.
  std::vector<std::string> args;
  std::vector<std::string> extraArgs;
  for (auto& a : argv) {
    if (!extraArgs.empty() || a == "--") {
      extraArgs.push_back(a);
    } else {
      args.push_back(a);
    }
  }
  // Tell the child to run in the foreground, to avoid fork bombing ourselves.
  args.push_back("--foreground");
  // We need to ensure that we pass down the log path, otherwise
  // getLogPath() will spot that we used --foreground and will pass an empty
  // logPath to this function.
  args.push_back("--logPath");
  args.push_back(logPath.str());

#ifndef _WIN32
  // If we started a privhelper, pass its control descriptor to the child
  if (privHelper && privHelper->getRawClientFd() != -1) {
    auto fd = opts.inheritDescriptor(FileDescriptor(
        ::dup(privHelper->getRawClientFd()), FileDescriptor::FDType::Socket));
    // Note: we can't use `--privhelper_fd=123` here because
    // startOrConnectToPrivHelper has an intentionally anemic argv parser.
    // It requires that the flag and the value be in separate
    // array entries.
    args.push_back("--privhelper_fd");
    args.push_back(folly::to<std::string>(fd));
  }
#endif

  // Set up a pipe for the child to pass back startup status
  Pipe exitStatusPipe;
  args.push_back("--startupLoggerFd");
  args.push_back(folly::to<std::string>(
      opts.inheritDescriptor(std::move(exitStatusPipe.write))));

  args.insert(args.end(), extraArgs.begin(), extraArgs.end());
  SpawnedProcess proc(args, std::move(opts));
  return ChildHandler{std::move(proc), std::move(exitStatusPipe.read)};
}

void DaemonStartupLogger::initClient(
    folly::StringPiece logPath,
    FileDescriptor&& pipe) {
#ifndef _WIN32
  // We call `setsid` on successful initialization,
  // but we need to call `setpgrp` early to make sure spawned processes
  // like `scribe_cat` belong to the same process group as the daemon process,
  // not the group of the process which initiated the eden start.
  // Note spawned processes are still not detached from the terminal,
  // which is incorrect.
  folly::checkUnixError(setpgid(0, 0), "setpgid failed");
#endif
  XDCHECK(!logPath.empty());
  pipe_ = std::move(pipe);
  redirectOutput(logPath);
}

void DaemonStartupLogger::runParentProcess(
    DaemonStartupLogger::ChildHandler&& child,
    folly::StringPiece logPath) {
  // Wait for the child to finish initializing itself and then exit
  // without ever returning to the caller.
  try {
    auto result =
        waitForChildStatus(child.exitStatusPipe, child.process, logPath);
    if (!result.errorMessage.empty()) {
      fprintf(stderr, "%s\n", result.errorMessage.c_str());
      fflush(stderr);
    }
    _exit(result.exitCode);
  } catch (const std::exception& ex) {
    // Catch exceptions to make sure we don't accidentally propagate them
    // out of spawn() in the parent process.
    fprintf(
        stderr,
        "unexpected error in daemonization parent process: %s\n",
        folly::exceptionStr(ex).c_str());
    fflush(stderr);
    _exit(EX_SOFTWARE);
  }
}

void DaemonStartupLogger::redirectOutput(StringPiece logPath) {
  try {
    logPath_ = logPath.str();

    // Save a copy of the original stderr descriptors, so we can still write
    // startup status messages directly to this descriptor.  This will be closed
    // once we complete initialization.
    origStderr_ = File(STDERR_FILENO, /*ownsFd=*/false).dupCloseOnExec();

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
    FileDescriptor& pipe,
    SpawnedProcess& proc,
    StringPiece logPath) {
  ResultType status;
  auto readResult = pipe.readFull(&status, sizeof(status));
  if (readResult.hasException()) {
    return ParentResult(
        EX_SOFTWARE,
        "error reading status of EdenFS initialization: ",
        folly::exceptionStr(readResult.exception()));
  }

  auto bytesRead = readResult.value();

  if (static_cast<size_t>(bytesRead) < sizeof(status)) {
    // This should only happen if edenfs crashed before writing its status.
    // Check to see if the child process has died.
    auto result = handleChildCrash(proc);
    result.errorMessage += folly::to<string>(
        "\nCheck the EdenFS log file at ", logPath, " for more details");
    return result;
  }

  // Return the status code.
  // The daemon process should have already printed a message about it status.
  return ParentResult(status);
}

DaemonStartupLogger::ParentResult DaemonStartupLogger::handleChildCrash(
    SpawnedProcess& proc) {
  constexpr size_t kMaxRetries = 5;
  constexpr auto kRetrySleep = 100ms;

  size_t numRetries = 0;
  while (true) {
    if (proc.terminated()) {
      auto status = proc.wait();
      if (status.killSignal() != 0) {
        return ParentResult(
            EX_SOFTWARE,
            "error: EdenFS crashed with status ",
            status.str(),
            " before it finished initializing");
      }
      auto exitCode = status.exitStatus();
      if (exitCode == 0) {
        // We don't ever want to exit successfully in this case, even if
        // the edenfs daemon somehow did.
        exitCode = EX_SOFTWARE;
      }
      return ParentResult(
          exitCode,
          "error: EdenFS ",
          status.str(),
          " before it finished initializing");
    }

    // The child hasn't actually exited yet.
    // Some of our tests appear to trigger this when killing the child with
    // SIGKILL.  We see the pipe closed before the child is waitable.
    // Sleep briefly and try the wait again, under the assumption that the
    // child will become waitable soon.
    if (numRetries < kMaxRetries) {
      ++numRetries;
      /* sleep override */ std::this_thread::sleep_for(kRetrySleep);
      continue;
    }

    // The child still wasn't waitable after waiting for a while.
    // This should only happen if there is a bug somehow.
    return ParentResult(
        EX_SOFTWARE,
        "error: EdenFS is still running but did not report "
        "its initialization status");
  }
}

void ForegroundStartupLogger::writeMessageImpl(folly::LogLevel, StringPiece) {}

void ForegroundStartupLogger::successImpl() {}

[[noreturn]] void ForegroundStartupLogger::failAndExitImpl(uint8_t exitCode) {
  exit(exitCode);
}

FileStartupLogger::FileStartupLogger(folly::StringPiece startupLogPath)
    : logFile_{
          startupLogPath,
          O_APPEND | O_CLOEXEC | O_CREAT | O_WRONLY,
          0644} {}

void FileStartupLogger::writeMessageImpl(
    folly::LogLevel,
    folly::StringPiece message) {
  writeMessageToFile(logFile_, message);
}

void FileStartupLogger::successImpl() {}

[[noreturn]] void FileStartupLogger::failAndExitImpl(uint8_t exitCode) {
  exit(exitCode);
}

namespace {

void writeMessageToFile(folly::File& file, folly::StringPiece message) {
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

} // namespace

} // namespace facebook::eden
