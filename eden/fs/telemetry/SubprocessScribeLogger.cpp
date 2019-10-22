/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/SubprocessScribeLogger.h"

#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>

using folly::Subprocess;

namespace {
/**
 * If the writer process is backed up, limit the message queue size to the
 * following bytes.
 */
constexpr size_t kQueueLimitBytes = 128 * 1024;

constexpr std::chrono::seconds kFlushTimeout{1};
constexpr std::chrono::seconds kProcessExitTimeout{1};
constexpr std::chrono::seconds kProcessTerminateTimeout{1};
} // namespace

namespace facebook {
namespace eden {

SubprocessScribeLogger::SubprocessScribeLogger(
    const char* executable,
    folly::StringPiece category)
    : SubprocessScribeLogger{
          std::vector<std::string>{executable, category.str()}} {}

SubprocessScribeLogger::SubprocessScribeLogger(
    const std::vector<std::string>& argv,
    int stdoutFd) {
  folly::File devnull{"/dev/null"};

  Subprocess::Options options;
  options.pipeStdin();
  options.stdoutFd(stdoutFd < 0 ? devnull.fd() : stdoutFd);
  // Forward stderr to the edenfs log.
  // Ensure that no cwd directory handles are held open.
  options.chdir("/");

  process_ = Subprocess{argv, options};

  SCOPE_FAIL {
    closeProcess();
  };

  writerThread_ = std::thread([this] {
    folly::setThreadName("ScribeLoggerWriter");
    writerThread();
  });
}

SubprocessScribeLogger::~SubprocessScribeLogger() {
  {
    auto state = state_.lock();
    state->shouldStop = true;
  }
  newMessageOrStop_.notify_one();

  {
    auto until = std::chrono::steady_clock::now() + kFlushTimeout;
    auto state = state_.lock();
    allMessagesWritten_.wait_until(
        state.getUniqueLock(), until, [&] { return state->didStop; });
  }

  closeProcess();
  writerThread_.join();
}

void SubprocessScribeLogger::closeProcess() {
  // Close the pipe, which should trigger the process to quit.
  process_.closeParentFd(STDIN_FILENO);

  // The writer thread might be blocked writing to a stuck process, so wait
  // until the process is dead to join the thread.
  process_.waitOrTerminateOrKill(kProcessExitTimeout, kProcessTerminateTimeout);
}

void SubprocessScribeLogger::log(std::string message) {
  size_t messageSize = message.size();

  {
    auto state = state_.lock();
    CHECK(!state->shouldStop) << "log() called during destruction - that's UB";
    if (state->didStop) {
      return;
    }
    if (state->totalBytes + messageSize > kQueueLimitBytes) {
      XLOG_EVERY_MS(DBG7, 10000) << "ScribeLogger queue full, dropping message";
      // queue full, dropping!
      return;
    }

    // This order is important in order to be atomic under std::bad_alloc.
    state->messages.emplace_back(std::move(message));
    state->totalBytes += messageSize;
  }
  newMessageOrStop_.notify_one();
}

void SubprocessScribeLogger::writerThread() {
  int fd = process_.stdinFd();

  for (;;) {
    std::string message;

    {
      auto state = state_.lock();
      newMessageOrStop_.wait(state.getUniqueLock(), [&] {
        return state->shouldStop || !state->messages.empty();
      });
      if (!state->messages.empty()) {
        CHECK_LE(state->messages.front().size(), state->totalBytes)
            << "totalSize accounting fell out of sync!";

        // The below statements are all noexcept.
        std::swap(message, state->messages.front());
        state->messages.pop_front();
        state->totalBytes -= message.size();
      } else {
        // If the predicate succeeded but we have no messages, then we're
        // shutting down cleanly.
        assert(state->shouldStop);
        CHECK_EQ(0, state->totalBytes)
            << "totalSize accounting fell out of sync!";
        state->didStop = true;
        state.unlock();
        allMessagesWritten_.notify_one();
        return;
      }
    }

    char newline = '\n';
    std::array<iovec, 2> iov;
    iov[0].iov_base = message.data();
    iov[0].iov_len = message.size();
    iov[1].iov_base = &newline;
    iov[1].iov_len = sizeof(newline);
    if (-1 == folly::writevNoInt(fd, iov.data(), iov.size())) {
      // TODO: We could attempt to restart the process here.
      XLOG(ERR) << "Failed to writev to logger process stdin: "
                << folly::errnoStr(errno) << ". Giving up!";
      // Give up. Allow the ScribeLogger class to be destroyed.
      {
        auto state = state_.lock();
        state->didStop = true;
        state->messages.clear();
        state->totalBytes = 0;
      }
      allMessagesWritten_.notify_one();
      return;
    }
  }
}

} // namespace eden
} // namespace facebook
