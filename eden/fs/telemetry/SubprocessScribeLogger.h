/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Subprocess.h>
#include <folly/Synchronized.h>
#include <list>
#include "eden/fs/telemetry/ScribeLogger.h"

namespace facebook {
namespace eden {

/**
 * SubprocessScribeLogger manages an external unix process and asynchronously
 * forwards newline-delimited messages to its stdin.
 */
class SubprocessScribeLogger : public ScribeLogger {
 public:
  /**
   * Launch `executable` with `category` as the first argument.
   */
  SubprocessScribeLogger(const char* executable, folly::StringPiece category);

  /**
   * Launch the process specified at argv[0] with the given argv, and forward
   * its stdout to `stdoutFd`, if non-negative. Otherwise, output goes to
   * /dev/null.
   */
  explicit SubprocessScribeLogger(
      const std::vector<std::string>& argv,
      int stdoutFd = -1);

  /**
   * Waits for the managed process to exit. If it is hung and doesn't complete,
   * terminates the process. Either way, this destructor will completed within a
   * bounded amount of time.
   */
  ~SubprocessScribeLogger();

  /**
   * Forwards a log message to the external process. Must not contain newlines,
   * since that is how the process distinguishes between messages.
   *
   * If the writer process is not keeping up, messages are dropped.
   */
  void log(std::string message) override;
  using ScribeLogger::log;

 private:
  void closeProcess();
  void writerThread();

  struct State {
    bool shouldStop = false;
    bool didStop = false;

    /// Sum of sizes of queued messages.
    size_t totalBytes = 0;
    /// Invariant: empty if didStop is true
    std::list<std::string> messages;
  };

  folly::Subprocess process_;
  std::thread writerThread_;

  folly::Synchronized<State, std::mutex> state_;
  std::condition_variable newMessageOrStop_;
  std::condition_variable allMessagesWritten_;
};

} // namespace eden
} // namespace facebook
