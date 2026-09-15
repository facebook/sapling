/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#if defined(__linux__) || defined(__APPLE__)

#include "eden/fs/service/PinScanRunner.h"

#include <fcntl.h>
#include <poll.h>
#include <unistd.h>

#include <algorithm>
#include <vector>

#include <folly/Exception.h>
#include <folly/ScopeGuard.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>

#include "eden/common/utils/SpawnedProcess.h"

namespace facebook::eden {

namespace {

constexpr auto kKillTimeout = std::chrono::milliseconds{250};
// Upper bound on how long a cancellation request can go unnoticed while
// waiting for scan output.
constexpr auto kPollSlice = std::chrono::milliseconds{100};
constexpr size_t kMaxOutput = 1024 * 1024;
// How much of each stream a failure event quotes.
constexpr size_t kPrefixBytes = 1024;

/**
 * Read what the non-blocking descriptor has, keeping the first `limit` bytes.
 * Returns 0, or the errno of a failed read.
 */
int drain(int fd, std::string& buffer, size_t limit, bool& eof) {
  while (true) {
    char buf[4096];
    auto n = ::read(fd, buf, sizeof(buf));
    if (n > 0) {
      if (buffer.size() < limit) {
        buffer.append(
            buf, std::min(static_cast<size_t>(n), limit - buffer.size()));
      }
      continue;
    }
    if (n == 0) {
      eof = true;
      return 0;
    }
    if (errno == EAGAIN || errno == EWOULDBLOCK) {
      return 0;
    }
    if (errno == EINTR) {
      continue;
    }
    return errno;
  }
}

} // namespace

folly::Expected<PinScanReport, PinScanFailure> runPinScan(
    const std::string& helperPath,
    const folly::CancellationToken& cancellationToken,
    std::chrono::milliseconds timeout) {
  const auto start = std::chrono::steady_clock::now();
  std::string output;
  std::string errors;
  auto fail = [&](std::string reason, std::string detail) {
    PinScanFailure failure{std::move(reason), std::move(detail)};
    failure.stdoutPrefix = output.substr(0, kPrefixBytes);
    failure.stderrPrefix = errors.substr(0, kPrefixBytes);
    failure.durationMs = std::chrono::duration_cast<std::chrono::milliseconds>(
                             std::chrono::steady_clock::now() - start)
                             .count();
    return folly::makeUnexpected(std::move(failure));
  };
  if (cancellationToken.isCancellationRequested()) {
    return fail("cancelled", "");
  }

  try {
    SpawnedProcess::Options options;
    options.pipeStdout();
    options.pipeStderr();
    options.nullStdin();
    SpawnedProcess proc(
        std::vector<std::string>{helperPath, "--scan-pins"},
        std::move(options));
    // SpawnedProcess aborts the process if destroyed before being waited on,
    // which unwinding to the catch below would otherwise do.
    SCOPE_FAIL {
      proc.terminateOrKill(kKillTimeout);
    };
    auto out = proc.stdoutFd();
    auto err = proc.stderrFd();
    for (int fd : {out.fd(), err.fd()}) {
      int flags = fcntl(fd, F_GETFL);
      folly::checkUnixError(fcntl(fd, F_SETFL, flags | O_NONBLOCK), "fcntl");
    }

    // stdout is the report; stderr is kept only as far as a failure quotes
    // it. Both are read until the child closes them.
    struct Stream {
      int fd{};
      std::string& buffer;
      size_t limit{};
      bool eof{false};
    };
    Stream streams[] = {
        {out.fd(), output, kMaxOutput + 1},
        {err.fd(), errors, kPrefixBytes},
    };
    const auto deadline = start + timeout;
    while (!streams[0].eof || !streams[1].eof) {
      if (cancellationToken.isCancellationRequested()) {
        proc.terminateOrKill(kKillTimeout);
        XLOG(DBG2, "pin scan cancelled");
        return fail("cancelled", "");
      }
      auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(
          deadline - std::chrono::steady_clock::now());
      if (remaining.count() <= 0) {
        proc.terminateOrKill(kKillTimeout);
        XLOG(WARN, "pin scan timed out; skipping directory invalidation");
        return fail("timeout", std::to_string(timeout.count()) + "ms");
      }
      // poll ignores a negative descriptor, which stands for a closed stream.
      struct pollfd pfds[2];
      for (size_t i = 0; i < 2; ++i) {
        pfds[i] = pollfd{streams[i].eof ? -1 : streams[i].fd, POLLIN, 0};
      }
      int pollResult = ::poll(
          pfds, 2, static_cast<int>(std::min(remaining, kPollSlice).count()));
      if (pollResult < 0 && errno != EINTR) {
        auto pollErrno = errno;
        proc.terminateOrKill(kKillTimeout);
        XLOGF(
            WARN,
            "pin scan poll failed: {}; skipping directory invalidation",
            folly::errnoStr(pollErrno));
        return fail("poll_error", folly::errnoStr(pollErrno));
      }
      if (pollResult <= 0) {
        continue;
      }
      for (size_t i = 0; i < 2; ++i) {
        if (pfds[i].fd < 0 || pfds[i].revents == 0) {
          continue;
        }
        auto& stream = streams[i];
        if (int readErrno =
                drain(stream.fd, stream.buffer, stream.limit, stream.eof)) {
          proc.terminateOrKill(kKillTimeout);
          XLOGF(
              WARN,
              "pin scan read failed: {}; skipping directory invalidation",
              folly::errnoStr(readErrno));
          return fail("read_error", folly::errnoStr(readErrno));
        }
      }
      if (output.size() > kMaxOutput) {
        proc.terminateOrKill(kKillTimeout);
        XLOG(WARN, "pin scan produced unreasonably large output");
        return fail(
            "output_too_large", std::to_string(output.size()) + " bytes");
      }
    }

    // The exit-status wait shares the read loop's deadline so the whole scan
    // is bounded by the timeout.
    auto status = proc.waitOrTerminateOrKill(
        std::max(
            std::chrono::duration_cast<std::chrono::milliseconds>(
                deadline - std::chrono::steady_clock::now()),
            std::chrono::milliseconds{0}),
        kKillTimeout);
    if (status.state() != ProcessStatus::Exited || status.exitStatus() != 0) {
      // A persistent failure mode is a privhelper binary that predates
      // --scan-pins, which rejects the flag and exits 1 every attempt, so
      // rate-limit the warning.
      XLOGF_EVERY_MS(
          WARN,
          60'000,
          "pin scan ({} --scan-pins) failed: {}: {}; "
          "skipping directory invalidation",
          helperPath,
          status.str(),
          folly::rtrimWhitespace(errors).str());
      return fail("exit_status", status.str());
    }
  } catch (const std::exception& ex) {
    XLOGF(
        WARN,
        "unable to run pin scan ({} --scan-pins): {}; "
        "skipping directory invalidation",
        helperPath,
        folly::exceptionStr(ex));
    return fail("spawn_error", folly::exceptionStr(ex).toStdString());
  }

  auto report = parsePinScanReport(output);
  if (!report) {
    XLOG(
        WARN,
        "pin scan output is malformed or incomplete; "
        "skipping directory invalidation");
    return fail("malformed_output", "");
  }
  return std::move(*report);
}

} // namespace facebook::eden

#endif // __linux__ || __APPLE__
