/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/privhelper/RestartSentinel.h"

#ifdef __APPLE__

#include <fcntl.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/json/json.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>
#include <sys/stat.h>
#include <algorithm>
#include <string_view>

namespace facebook::eden {

namespace {
// A command line and an environment, generously. The sentinel is written by an
// unprivileged process, so its size is bounded before anything is parsed.
constexpr size_t kMaxSentinelSize = 1024 * 1024;

// The restart policy arrives over IPC from the unprivileged daemon, so the
// privhelper bounds what it will honour. The daemon's own defaults are 3
// restarts per 10 minutes.
constexpr uint32_t kRestartsCeiling = 10;
constexpr uint32_t kMinRestartWindowSeconds = 60;
constexpr uint32_t kMaxRestartWindowSeconds = 24 * 60 * 60;

// Stands in for the one sentinel resolution failure that has no errno.
constexpr int kMalformedSentinelPath = 0;

int openatNoInt(int dirFd, const char* name, int flags) {
  int fd;
  do {
    fd = ::openat(dirFd, name, flags);
  } while (fd == -1 && errno == EINTR);
  return fd;
}

struct SentinelPathParts {
  std::string_view dir;
  std::string_view name;
};

/**
 * An absolute sentinel path split into the directory to pin and the leaf to
 * look up in it, or nullopt when the path cannot name a file: "." and ".."
 * always resolve, so faccessat() could never report the sentinel gone, and a
 * NUL ends the path the syscalls act on early.
 */
std::optional<SentinelPathParts> splitSentinelPath(const std::string& path) {
  if (path.empty() || path.front() != '/' ||
      path.find('\0') != std::string::npos) {
    return std::nullopt;
  }
  const auto view = std::string_view{path};
  const auto slash = view.rfind('/');
  const auto name = view.substr(slash + 1);
  if (name.empty() || name == "." || name == "..") {
    return std::nullopt;
  }
  return SentinelPathParts{view.substr(0, slash == 0 ? 1 : slash), name};
}
} // namespace

void RestartSentinel::setConfig(EdenFsRestartArgs args) {
  config_ = std::move(args);
  location_.reset();
  lastResolutionError_.reset();
  // A daemon that resends its configuration has recovered from a failed
  // takeover, and would otherwise stay permanently un-restartable behind the
  // flag its aborted shutdown set.
  cleanShutdownNotified_ = false;
}

void RestartSentinel::noteCleanShutdown() {
  cleanShutdownNotified_ = true;
}

bool RestartSentinel::enabled() const {
  return config_.has_value() && config_->enabled;
}

uint32_t RestartSentinel::restartCount() const {
  return config_.value().restartCount;
}

uint64_t RestartSentinel::firstRestartEpochSec() const {
  return config_.value().firstRestartEpochSec;
}

const RestartSentinel::Location* RestartSentinel::location() const {
  if (location_.has_value()) {
    return &*location_;
  }
  if (!config_.has_value()) {
    return nullptr;
  }
  // A retry can fail for a new reason, so only an exact repeat is dropped.
  const auto isNewFailure = [this](int error) {
    const bool changed = lastResolutionError_ != error;
    lastResolutionError_ = error;
    return changed;
  };

  const auto& path = config_->sentinelPath;
  const auto parts = splitSentinelPath(path);
  if (!parts.has_value()) {
    if (isNewFailure(kMalformedSentinelPath)) {
      XLOGF(ERR, "the restart sentinel path {} does not name a file", path);
    }
    return nullptr;
  }

  const auto dir = std::string{parts->dir};
  // O_DIRECTORY rejects a FIFO or a device planted where the state directory
  // should be; O_NOFOLLOW rejects a symlink as the final component, though the
  // ancestors above it are still resolved as root.
  const int fd = folly::openNoInt(
      dir.c_str(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
  if (fd == -1) {
    const int error = errno;
    if (isNewFailure(error)) {
      XLOGF(
          ERR,
          "cannot open the restart sentinel's directory {}: {}",
          dir,
          folly::errnoStr(error));
    }
    return nullptr;
  }

  location_ =
      Location{folly::File{fd, /*ownsFd=*/true}, std::string{parts->name}};
  return &*location_;
}

std::optional<RestartSentinel::DisarmState> RestartSentinel::disarmState()
    const {
  if (!config_.has_value()) {
    return std::nullopt;
  }
  if (cleanShutdownNotified_) {
    return DisarmState::ShutdownAnnounced;
  }
  const auto* loc = location();
  if (loc == nullptr) {
    return DisarmState::Unknown;
  }
  // The second, independent disarm signal. Only existence matters, so
  // faccessat() rather than an open: a FIFO planted in the sentinel's place
  // would block an open for ever. A spoofed "exists" only buys a restart a
  // crash also buys.
  if (::faccessat(loc->dir.fd(), loc->name.c_str(), F_OK, 0) == 0) {
    return DisarmState::Armed;
  }
  const int error = errno;
  // Only ENOENT means the daemon removed it. Anything else leaves the
  // sentinel's state unknown, and root must not relaunch on a guess.
  if (error != ENOENT) {
    XLOGF(
        ERR,
        "cannot read the restart sentinel {} in the pinned directory: {}",
        loc->name,
        folly::errnoStr(error));
    return DisarmState::Unknown;
  }
  return DisarmState::ShutdownAnnounced;
}

std::optional<RestartSentinel::RelaunchCommand>
RestartSentinel::readRelaunchCommand() const {
  if (!config_.has_value()) {
    return std::nullopt;
  }
  // 0 is what an absent nonce parses to, so a configuration carrying it would
  // accept a sentinel written by a daemon too old to have one.
  if (config_->sentinelNonce == 0) {
    XLOGF(ERR, "not restarting edenfs: the restart configuration has no nonce");
    return std::nullopt;
  }
  const auto* loc = location();
  if (loc == nullptr) {
    return std::nullopt;
  }

  // A root process reading a file the daemon's user can replace: O_NOFOLLOW
  // rejects a symlink swapped in for the sentinel, and O_NONBLOCK keeps a FIFO
  // from blocking here so the regular-file check below can reject it.
  const int fd = openatNoInt(
      loc->dir.fd(),
      loc->name.c_str(),
      O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK);
  if (fd == -1) {
    XLOGF(
        ERR,
        "not restarting edenfs: cannot open the restart sentinel {} in the pinned directory: {}",
        loc->name,
        folly::errnoStr(errno));
    return std::nullopt;
  }
  const folly::File sentinel{fd, /*ownsFd=*/true};

  struct stat st{};
  if (::fstat(sentinel.fd(), &st) != 0) {
    XLOGF(
        ERR,
        "not restarting edenfs: cannot stat the restart sentinel {}: {}",
        loc->name,
        folly::errnoStr(errno));
    return std::nullopt;
  }
  if (!S_ISREG(st.st_mode)) {
    XLOGF(
        ERR,
        "not restarting edenfs: the restart sentinel {} is not a regular file",
        loc->name);
    return std::nullopt;
  }
  // Privileges are dropped to uid_ before the command runs, so whoever can
  // write this file picks what runs as the daemon's user. The path is
  // caller-supplied, so the rejection does not name the file's uid or mode.
  if (st.st_uid != uid_ || (st.st_mode & (S_IWGRP | S_IWOTH)) != 0) {
    XLOGF(
        ERR,
        "not restarting edenfs: the restart sentinel {} has wrong ownership",
        loc->name);
    return std::nullopt;
  }
  if (st.st_size <= 0 || static_cast<size_t>(st.st_size) > kMaxSentinelSize) {
    XLOGF(
        ERR,
        "not restarting edenfs: the restart sentinel {} is {} bytes",
        loc->name,
        st.st_size);
    return std::nullopt;
  }

  std::string contents;
  if (!folly::readFile(sentinel.fd(), contents, kMaxSentinelSize)) {
    XLOGF(
        ERR,
        "not restarting edenfs: cannot read the restart sentinel {}: {}",
        loc->name,
        folly::errnoStr(errno));
    return std::nullopt;
  }

  // Written by EdenServer::armPrivHelperRestart(); the shape is fixed:
  //
  //   {"argv": ["...", ...], "env": {"KEY": "VALUE", ...}, "nonce": 123}
  RelaunchCommand command;
  try {
    const auto parsed = folly::parseJson(contents);

    // The sentinel path is fixed per state dir, so this privhelper may open a
    // file a newer generation wrote. A sentinel with no nonce reads as 0,
    // which no generation ever stamps.
    const auto* nonce = parsed.get_ptr("nonce");
    const uint64_t sentinelNonce =
        nonce && nonce->isInt() ? static_cast<uint64_t>(nonce->asInt()) : 0;
    if (sentinelNonce != config_->sentinelNonce) {
      XLOGF(
          ERR,
          "not restarting edenfs: the restart sentinel {} belongs to another "
          "daemon generation",
          loc->name);
      return std::nullopt;
    }

    const auto* argv = parsed.get_ptr("argv");
    if (!argv || !argv->isArray() || argv->empty()) {
      XLOGF(
          ERR,
          "not restarting edenfs: the restart sentinel {} holds no command",
          loc->name);
      return std::nullopt;
    }
    for (const auto& arg : *argv) {
      command.argv.push_back(arg.asString());
    }
    if (const auto* env = parsed.get_ptr("env"); env && env->isObject()) {
      for (const auto& [key, value] : env->items()) {
        command.env.emplace_back(key.asString(), value.asString());
      }
    }
  } catch (const std::exception&) {
    // Without the exception's message: folly's JSON errors quote the offending
    // input, and these are somebody else's file contents in a root process's
    // log.
    XLOGF(ERR, "not restarting edenfs: invalid restart sentinel {}", loc->name);
    return std::nullopt;
  }

  return command;
}

bool RestartSentinel::admitRestartAttempt(uint64_t now) {
  auto& config = config_.value();
  // Neither value is trusted: a window of zero would reset the count on every
  // attempt, and maxRestarts could arrive as UINT32_MAX.
  const uint64_t window = std::clamp(
      config.windowSeconds, kMinRestartWindowSeconds, kMaxRestartWindowSeconds);
  const uint32_t maxRestarts = std::min(config.maxRestarts, kRestartsCeiling);

  // A clock that moved backwards is treated as a fresh window rather than
  // wrapping the unsigned subtraction into a huge number.
  if (config.firstRestartEpochSec == 0 || now < config.firstRestartEpochSec ||
      now - config.firstRestartEpochSec > window) {
    config.restartCount = 0;
    config.firstRestartEpochSec = now;
  }

  if (config.restartCount >= maxRestarts) {
    XLOGF(
        WARN,
        "not restarting edenfs: already restarted it {} times within {}s",
        config.restartCount,
        window);
    return false;
  }

  ++config.restartCount;
  return true;
}

} // namespace facebook::eden

#endif // __APPLE__
