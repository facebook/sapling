/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

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

} // namespace

std::optional<PinScanReport> runPinScan(
    const std::string& helperPath,
    const folly::CancellationToken& cancellationToken,
    std::chrono::milliseconds timeout) {
  if (cancellationToken.isCancellationRequested()) {
    return std::nullopt;
  }

  std::string output;
  try {
    SpawnedProcess::Options options;
    options.pipeStdout();
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
    int flags = fcntl(out.fd(), F_GETFL);
    folly::checkUnixError(
        fcntl(out.fd(), F_SETFL, flags | O_NONBLOCK), "fcntl");

    const auto deadline = std::chrono::steady_clock::now() + timeout;
    bool eof = false;
    while (!eof) {
      if (cancellationToken.isCancellationRequested()) {
        proc.terminateOrKill(kKillTimeout);
        XLOG(DBG2, "pin scan cancelled");
        return std::nullopt;
      }
      auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(
          deadline - std::chrono::steady_clock::now());
      if (remaining.count() <= 0) {
        proc.terminateOrKill(kKillTimeout);
        XLOG(WARN, "pin scan timed out; skipping directory invalidation");
        return std::nullopt;
      }
      struct pollfd pfd{out.fd(), POLLIN, 0};
      int pollResult = ::poll(
          &pfd, 1, static_cast<int>(std::min(remaining, kPollSlice).count()));
      if (pollResult < 0 && errno != EINTR) {
        auto err = errno;
        proc.terminateOrKill(kKillTimeout);
        XLOGF(
            WARN,
            "pin scan poll failed: {}; skipping directory invalidation",
            folly::errnoStr(err));
        return std::nullopt;
      }
      if (pollResult <= 0) {
        continue;
      }
      while (true) {
        char buf[4096];
        auto n = ::read(out.fd(), buf, sizeof(buf));
        if (n > 0) {
          output.append(buf, n);
          if (output.size() > kMaxOutput) {
            proc.terminateOrKill(kKillTimeout);
            XLOG(WARN, "pin scan produced unreasonably large output");
            return std::nullopt;
          }
          continue;
        }
        if (n == 0) {
          eof = true;
          break;
        }
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
          break;
        }
        if (errno == EINTR) {
          continue;
        }
        auto err = errno;
        proc.terminateOrKill(kKillTimeout);
        XLOGF(
            WARN,
            "pin scan read failed: {}; skipping directory invalidation",
            folly::errnoStr(err));
        return std::nullopt;
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
          "pin scan ({} --scan-pins) failed: {}; "
          "skipping directory invalidation",
          helperPath,
          status.str());
      return std::nullopt;
    }
  } catch (const std::exception& ex) {
    XLOGF(
        WARN,
        "unable to run pin scan ({} --scan-pins): {}; "
        "skipping directory invalidation",
        helperPath,
        folly::exceptionStr(ex));
    return std::nullopt;
  }

  auto report = parsePinScanReport(output);
  if (!report) {
    XLOG(
        WARN,
        "pin scan output is malformed or incomplete; "
        "skipping directory invalidation");
    return std::nullopt;
  }
  return report;
}

} // namespace facebook::eden

#endif // __linux__
