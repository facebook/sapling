/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <fmt/ranges.h>
#include <folly/File.h>
#include <folly/Range.h>
#include <folly/lang/Assume.h>
#include <folly/logging/LogLevel.h>
#include <folly/portability/GFlags.h>
#include <memory>
#include <optional>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/utils/FileDescriptor.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SpawnedProcess.h"

namespace facebook::eden {

DECLARE_int32(startupLoggerFd);

class StartupLogger;
class PrivHelper;
class FileDescriptor;

/**
 * daemonizeIfRequested manages optionally daemonizing the edenfs process.
 * Daemonizing is controlled primarily by the `--foreground` command line
 * argument NOT being present, and on Windows systems we don't currently
 * daemonize, but could do so now that we no longer rely on `fork` to
 * implement this feature.
 *
 * If daemonizing: this function will configure a channel to communicate
 * with the child process so that the parent can tell when it has finished
 * initializing.  The parent will then call into
 * DaemonStartupLogger::runParentProcess which waits for initialization
 * to complete, prints the status and then terminates.  This function will
 * therefore never return in the parent process.
 *
 * In the child process spawned as part of daemonizing, `--startupLoggerFd`
 * is passed as a command line argument and the child will use that file
 * descriptor to set up a client to communicate status with the parent.
 * This function will return a `StartupLogger` instance in the child to
 * manage that state.
 *
 * In the non-daemonizing case, no child is spawned and this function
 * will return a `StartupLogger` that simply writes to the configured
 * log location.
 */
std::shared_ptr<StartupLogger> daemonizeIfRequested(
    folly::StringPiece logPath,
    PrivHelper* privHelper,
    const std::vector<std::string>& argv);

/**
 * StartupLogger provides an API for logging messages that should be displayed
 * to the user while edenfs is starting.
 *
 * If edenfs is daemonizing, the original foreground process will not exit until
 * success() or fail() is called.  Any messages logged with log() or warn() will
 * be shown printed in the original foreground process.
 */
class StartupLogger {
 public:
  virtual ~StartupLogger();

  /**
   * Log an informational message.
   *
   * Note that it is valid to call log() even after success() has been called.
   * This can occur if edenfs has been asked to report successful startup
   * without waiting for all mount points to be remounted.
   */
  template <typename... Args>
  void log(const Args&... args) {
    writeMessage(
        folly::LogLevel::DBG2,
        fmt::to_string(
            fmt::join(std::make_tuple<const Args&...>(args...), "")));
  }

  /**
   * Log a verbose message
   */
  template <typename... Args>
  void logVerbose(const Args&... args) {
    writeMessage(
        folly::LogLevel::DBG7,
        fmt::to_string(
            fmt::join(std::make_tuple<const Args&...>(args...), "")));
  }

  /**
   * Log a warning message.
   */
  template <typename... Args>
  void warn(const Args&... args) {
    writeMessage(
        folly::LogLevel::WARN,
        fmt::to_string(
            fmt::join(std::make_tuple<const Args&...>(args...), "")));
  }

  /**
   * Indicate that startup has failed.
   *
   * This exits the current process, and also causes the original foreground
   * process to exit if edenfs has daemonized.
   */
  template <typename... Args>
  [[noreturn]] void exitUnsuccessfully(uint8_t exitCode, Args&&... args) {
    writeMessage(
        folly::LogLevel::ERR,
        folly::to<std::string>(std::forward<Args>(args)...));
    failAndExitImpl(exitCode);
    folly::assume_unreachable();
  }

  /**
   * Indicate that startup has succeeded.
   *
   * If edenfs has daemonized this will cause the original foreground edenfs
   * process to exit successfully.
   */
  void success(uint64_t startTimeInSeconds);

 protected:
  void writeMessage(folly::LogLevel level, folly::StringPiece message);

  virtual void writeMessageImpl(
      folly::LogLevel level,
      folly::StringPiece message) = 0;
  virtual void successImpl() = 0;
  [[noreturn]] virtual void failAndExitImpl(uint8_t exitCode) = 0;
};

class DaemonStartupLogger : public StartupLogger {
 public:
  DaemonStartupLogger() = default;

  /**
   * Spawn a child process to act as the server.
   *
   * This method will never return.
   * It spawns a child process and then waits for the child to either call
   * StartupLogger::success() or
   * StartupLogger::fail(), and exits with a status code based on which of these
   * was called.
   *
   * If logPath is non-empty the child process will redirect its stdout and
   * stderr file descriptors to the specified log file before returning.
   */
  [[noreturn]] void spawn(
      folly::StringPiece logPath,
      PrivHelper* privHelper,
      const std::vector<std::string>& argv);

  /** Configure the logger to act as a client of it parent.
   * `pipe` is the file descriptor passed down via `--startupLoggerFd`
   * and is connected to the parent process which is waiting in the
   * `spawn`/`runParentProcess` method.
   * This method configures this startup logger for the child so that it
   * can communicate the status with the parent.
   */
  void initClient(folly::StringPiece logPath, FileDescriptor&& pipe);

 protected:
  void writeMessageImpl(folly::LogLevel level, folly::StringPiece message)
      override;
  void successImpl() override;
  [[noreturn]] void failAndExitImpl(uint8_t exitCode) override;

 private:
  friend class DaemonStartupLoggerTest;

  using ResultType = uint8_t;

  struct ParentResult {
    template <typename... Args>
    explicit ParentResult(uint8_t code, Args&&... args)
        : exitCode(code),
          errorMessage(folly::to<std::string>(std::forward<Args>(args)...)) {}

    int exitCode;
    std::string errorMessage;
  };

  /*
   * On Windows, we can't share stderr of the parent process to the daemon
   * process, as the daemon will terminate once the console is closed.  As a
   * result, we will be redirecting the stderr output from the daemon to a pipe,
   * then spawn a new thread to write it to the parent process's stderr until
   * the startup process is finished.
   *
   * This struct manages that redirection thread.
   */
  struct ChildHandler {
    ChildHandler(SpawnedProcess&& process, FileDescriptor exitStatusPipe);

    ~ChildHandler();

    ChildHandler(const ChildHandler&) = delete;
    ChildHandler& operator=(const ChildHandler&) = delete;

    ChildHandler(ChildHandler&& other) noexcept = delete;
    ChildHandler& operator=(ChildHandler&& other) noexcept = delete;

    SpawnedProcess process;
    FileDescriptor exitStatusPipe;

   private:
    std::thread stderrBridge_;
  };

  ChildHandler spawnImpl(
      folly::StringPiece logPath,
      PrivHelper* privHelper,
      const std::vector<std::string>& argv);

  [[noreturn]] void runParentProcess(
      ChildHandler&& child,
      folly::StringPiece logPath);
  void redirectOutput(folly::StringPiece logPath);

  /**
   * Wait for the child process to write its initialization status.
   */
  ParentResult waitForChildStatus(
      FileDescriptor& pipe,
      SpawnedProcess& proc,
      folly::StringPiece logPath);
  ParentResult handleChildCrash(SpawnedProcess& childPid);

  void sendResult(ResultType result);

  // If stderr has been redirected during process daemonization, origStderr_
  // contains a file descriptor referencing the original stderr.  It is used to
  // continue to print informational messages directly to the user during
  // startup even after normal log redirection.
  //
  // If log redirection has not occurred these will simply be closed File
  // objects.  The normal logging mechanism is sufficient to show messages to
  // the user in this case.
  folly::File origStderr_;
  std::string logPath_;

  // If we have daemonized, pipe_ is a pipe connected to the original foreground
  // process.  We use this to inform the original process when we have fully
  // completed daemon startup.
  FileDescriptor pipe_;
};

class ForegroundStartupLogger : public StartupLogger {
 public:
  ForegroundStartupLogger() = default;

 protected:
  void writeMessageImpl(folly::LogLevel level, folly::StringPiece message)
      override;
  void successImpl() override;
  [[noreturn]] void failAndExitImpl(uint8_t exitCode) override;
};

class FileStartupLogger : public StartupLogger {
 public:
  explicit FileStartupLogger(folly::StringPiece startupLogFile);

 protected:
  void writeMessageImpl(folly::LogLevel level, folly::StringPiece message)
      override;
  void successImpl() override;
  [[noreturn]] void failAndExitImpl(uint8_t exitCode) override;

  folly::File logFile_;
};

} // namespace facebook::eden
