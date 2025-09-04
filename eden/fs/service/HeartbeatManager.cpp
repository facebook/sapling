/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/HeartbeatManager.h"

#include <fcntl.h>
#include <chrono>
#include <filesystem>

#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>

#ifdef __APPLE__
#include <sys/sysctl.h>
#include <sys/time.h>
#include <unistd.h>
#endif

#include "eden/common/telemetry/StructuredLogger.h"
#include "eden/common/utils/FileUtils.h"
#include "eden/fs/telemetry/LogEvent.h"

using std::optional;
using std::string;
using std::unique_ptr;

namespace facebook::eden {

namespace {

constexpr const char* kDaemonExitSignalFileName = "daemon_exit_signal";

#ifdef __APPLE__
time_t getBootTimeSysctl() {
  struct timeval boottime;
  size_t size = sizeof(boottime);
  int mib[2] = {CTL_KERN, KERN_BOOTTIME};
  if (sysctl(mib, 2, &boottime, &size, nullptr, 0) < 0) {
    // It cannot find the last system boot time, just return -1
    return -1;
  }
  return boottime.tv_sec;
}
#endif

} // namespace

HeartbeatManager::HeartbeatManager(
    const EdenStateDir& edenDir,
    std::shared_ptr<StructuredLogger> structuredLogger)
    : edenDir_(edenDir),
      structuredLogger_(std::move(structuredLogger)),
      heartbeatFilePath_(
          edenDir_.getPath() + PathComponentPiece{getHeartbeatFileName()}),
      heartbeatFilePathString_(heartbeatFilePath_.c_str()),
      daemonExitSignalFilePath_(
          edenDir_.getPath() + PathComponentPiece{kDaemonExitSignalFileName}),
      daemonExitSignalFilePathString_(daemonExitSignalFilePath_.c_str()) {}

void HeartbeatManager::createOrUpdateHeartbeatFile() {
#ifndef _WIN32
  // Create the heartbeat file and write the current timestamp to it
  auto now = std::chrono::system_clock::now();
  auto now_c = std::chrono::system_clock::to_time_t(now);
  std::string now_str = std::to_string(now_c);
  auto result =
      writeFileAtomic(heartbeatFilePath_, folly::StringPiece(now_str));
  if (result.hasException()) {
    XLOGF(
        ERR,
        "Failed to create or update heartbeat flag file: {}",
        result.exception().what());
  }
#endif
}

void HeartbeatManager::removeHeartbeatFile() {
#ifndef _WIN32
  const int rc = unlink(heartbeatFilePathString_.c_str());
  if (rc != 0 && errno != ENOENT) {
    XLOGF(ERR, "Failed to remove eden heartbeat file: {}", errno);
  }
  removeDaemonExitSignalFile();
#endif
}

bool HeartbeatManager::checkForPreviousHeartbeat(
    bool takeover,
    const std::optional<std::string>& oldEdenHeartbeatFileNameStr) {
#ifdef _WIN32
  return false;
#else
  bool crashDetected = false;
  folly::StringPiece heartbeatFileNamePrefix =
      edenDir_.getHeartbeatFileNamePrefix();
  std::string edenHeartbeatPathFileNameStr = getHeartbeatFileName();

  // Check if the previous eden has crashed
  for (const auto& entry :
       std::filesystem::directory_iterator(edenDir_.getPath().asString())) {
    if (entry.is_regular_file() &&
        entry.path().filename().string().starts_with(
            heartbeatFileNamePrefix.toString())) {
      if (oldEdenHeartbeatFileNameStr.has_value() && takeover &&
          entry.path().filename().string() ==
              oldEdenHeartbeatFileNameStr.value()) {
        // We have a heartbeat file from the previous eden. But it is
        // not a crash because eden is taking over the previous eden
        // during graceful restart. This heartbeat file will be
        // deleted when the previous eden cleanups.
        continue;
      } else if (
          entry.path().filename().string() == edenHeartbeatPathFileNameStr) {
        // We have a heartbeat file but it is from the current eden.
        // It could happen during graceful restart when takeover fail
        // and it fallback to the previous eden. We should not delete
        // the heartbeat file in this case.
        continue;
      } else {
        // Read the latest timestamp from the heartbeat file
        std::string latestDaemonHeartbeatStr;
        uint64_t latestDaemonHeartbeat = 0;
        uint8_t daemon_exit_signal = 0;
        if (folly::readFile(
                entry.path().string().c_str(), latestDaemonHeartbeatStr)) {
          // Convert latestDaemonHeartbeatStr to uint64_t
          latestDaemonHeartbeat = folly::to<uint64_t>(latestDaemonHeartbeatStr);
        }
        // read the exit signal from daemon_exit_signal file if it
        // exists
        daemon_exit_signal = readDaemonExitSignal();
        XLOGF(
            ERR,
            "ERROR: The previous edenFS daemon exited silently with signal {}",
            daemon_exit_signal == 0 ? "Unknown"
                                    : std::to_string(daemon_exit_signal));
#ifdef __APPLE__
        time_t bootTime = getBootTimeSysctl();
#else
        time_t bootTime = 0;
#endif
        // Log a crash event
        structuredLogger_->logEvent(SilentDaemonExit{
            latestDaemonHeartbeat,
            daemon_exit_signal,
            static_cast<uint64_t>(bootTime)});

        std::remove(entry.path().string().c_str());
        // Remove any existing daemon exit signal file to clean up
        // signals for the new edenFS daemon
        removeDaemonExitSignalFile();
        crashDetected = true;
      }
    }
  }

  return crashDetected;
#endif
}

#ifndef _WIN32
void HeartbeatManager::createDaemonExitSignalFile(int signal) {
  // Create the daemon exit signal file and write the signal to it
  // createDaemonExitSignalFile() should be an async-signal-safe function.
  // It get called from signal handlers. Full rules:
  // https://man7.org/linux/man-pages/man7/signal-safety.7.html
  int fileno = open(
      daemonExitSignalFilePathString_.c_str(),
      O_WRONLY | O_CREAT | O_TRUNC,
      0644);
  if (fileno == -1) {
    return;
  }
  char buf[10];
  int str_len = intToStrSafe(signal, buf, sizeof(buf));
  write(fileno, buf, str_len);
  close(fileno);
}

void HeartbeatManager::removeDaemonExitSignalFile() {
  // Remove the daemon exit signal file if it exists
  const int rc = unlink(daemonExitSignalFilePathString_.c_str());
  if (rc != 0 && errno != ENOENT) {
    XLOGF(ERR, "Failed to remove daemon exit signal file: {}", errno);
  }
}
#endif

int HeartbeatManager::readDaemonExitSignal() {
#ifdef _WIN32
  return 0;
#else
  // Read the signal from the daemon exit signal file
  std::string signalStr;
  if (folly::readFile(daemonExitSignalFilePathString_.c_str(), signalStr)) {
    // Optionally trim whitespace/newlines
    folly::trimWhitespace(signalStr);
    try {
      return folly::to<uint8_t>(signalStr);
    } catch (const std::exception&) {
      return 0;
    }
  }
  // File does not exist or is empty, return 0
  return 0;
#endif
}

std::string HeartbeatManager::getHeartbeatFileName() const {
  const auto pidContents = folly::to<std::string>(getpid());
  return edenDir_.getHeartbeatFileNamePrefix().toString() + pidContents;
}

// Convert integer to string in a signal-safe way (simple itoa)
// return the length of the string
int HeartbeatManager::intToStrSafe(int val, char* buf, size_t buf_size) {
  if (buf_size == 0) {
    return 0;
  }
  size_t i = buf_size - 1;
  buf[i] = '\0';
  i--;
  unsigned int v;
  bool negative = false;
  if (val < 0) {
    negative = true;
    v = -val;
  } else {
    v = val;
  }
  if (v == 0) {
    buf[i--] = '0';
  } else {
    while (v > 0 && i > 0) {
      buf[i--] = '0' + (v % 10);
      v /= 10;
    }
  }
  if (negative && i > 0) {
    buf[i--] = '-';
  }
  // Shift string to start of buffer
  size_t start = i + 1;
  int j = 0;
  while (buf[start] != '\0') {
    buf[j++] = buf[start++];
  }
  buf[j] = '\0';
  return j;
}

} // namespace facebook::eden
