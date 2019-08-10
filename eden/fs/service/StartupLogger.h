/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Conv.h>
#include <folly/File.h>
#include <folly/Range.h>
#include <folly/lang/Assume.h>
#include <folly/logging/LogLevel.h>
#include <gflags/gflags_declare.h>
#include <memory>
#include <optional>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class StartupLogger;

std::unique_ptr<StartupLogger> daemonizeIfRequested(folly::StringPiece logPath);

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
  void log(Args&&... args) {
    writeMessage(
        folly::LogLevel::DBG2,
        folly::to<std::string>(std::forward<Args>(args)...));
  }

  /**
   * Log a warning message.
   */
  template <typename... Args>
  void warn(Args&&... args) {
    writeMessage(
        folly::LogLevel::WARN,
        folly::to<std::string>(std::forward<Args>(args)...));
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
  void success();

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
   * daemonize the current process.
   *
   * This method returns in a new process.  This method will never return in the
   * parent process that originally called daemonize().  Instead the parent
   * waits for the child process to either call StartupLogger::success() or
   * StartupLogger::fail(), and exits with a status code based on which of these
   * was called.
   *
   * If logPath is non-empty the child process will redirect its stdout and
   * stderr file descriptors to the specified log file before returning.
   */
  void daemonize(folly::StringPiece logPath);

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

  std::optional<std::pair<pid_t, folly::File>> daemonizeImpl(
      folly::StringPiece logPath);

  /**
   * Create the pipe for communication between the parent process and the
   * daemonized child.  Stores the write end in pipe_ and returns the read end.
   */
  folly::File createPipe();

  [[noreturn]] void runParentProcess(
      folly::File readPipe,
      pid_t childPid,
      folly::StringPiece logPath);
  void prepareChildProcess(folly::StringPiece logPath);
  void closeUndesiredInheritedFileDescriptors();
  void redirectOutput(folly::StringPiece logPath);

  /**
   * Wait for the child process to write its initialization status.
   */
  ParentResult waitForChildStatus(
      const folly::File& pipe,
      pid_t childPid,
      folly::StringPiece logPath);
  ParentResult handleChildCrash(pid_t childPid);

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
  folly::File pipe_;
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

} // namespace eden
} // namespace facebook
