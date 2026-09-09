/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/privhelper/RestartSentinel.h"

#ifdef __APPLE__

#include <fcntl.h>
#include <folly/Expected.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/Unit.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>
#include <sys/stat.h>
#include <algorithm>
#include <string_view>
#include <utility>

namespace facebook::eden {

namespace {
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
 * always resolve, so the sentinel could never be reported gone, and a NUL ends
 * the path the syscalls act on early.
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

/** Why the sentinel is not a marker the daemon's user could have created. */
enum class SentinelError {
  /** Nothing is at the name. */
  Absent,
  /** Something is at the name, and it is not such a marker. */
  Rejected,
  /** What is at the name could not be established. */
  Indeterminate,
};

/**
 * Open the sentinel and establish that it is plausibly a marker the daemon's
 * user created: a regular file owned by `uid` that only its owner can write.
 *
 * Runs as root against a name an unprivileged user controls, on a path that
 * still owes the mounts a cleanup, so it neither blocks nor throws.
 *
 * Logs every failure but `Absent`, naming the consequence: a sentinel that is
 * simply gone is the ordinary clean-shutdown signal, and every other reason
 * leaves edenfs down.
 */
folly::Expected<folly::Unit, SentinelError>
openSentinel(int dirFd, const std::string& name, uid_t uid) {
  // O_NOFOLLOW rejects a symlink swapped in for the sentinel, and O_NONBLOCK
  // keeps a FIFO from blocking here so the regular-file check below can reject
  // it.
  const int fd = openatNoInt(
      dirFd, name.c_str(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK);
  if (fd == -1) {
    const int error = errno;
    if (error == ENOENT) {
      return folly::makeUnexpected(SentinelError::Absent);
    }
    XLOGF(
        ERR,
        "not restarting edenfs: cannot open the restart sentinel {} in the pinned directory: {}",
        name,
        folly::errnoStr(error));
    // ELOOP is O_NOFOLLOW refusing a symlink, which settles what is at the
    // name. Every other errno means the name could not be examined at all.
    return folly::makeUnexpected(
        error == ELOOP ? SentinelError::Rejected
                       : SentinelError::Indeterminate);
  }
  folly::File file{fd, /*ownsFd=*/true};

  struct stat st{};
  if (::fstat(file.fd(), &st) != 0) {
    XLOGF(
        ERR,
        "not restarting edenfs: cannot stat the restart sentinel {}: {}",
        name,
        folly::errnoStr(errno));
    return folly::makeUnexpected(SentinelError::Indeterminate);
  }
  if (!S_ISREG(st.st_mode)) {
    XLOGF(
        ERR,
        "not restarting edenfs: the restart sentinel {} is not a regular file",
        name);
    return folly::makeUnexpected(SentinelError::Rejected);
  }
  // Its existence is what keeps root armed, so whoever can create this file
  // can force a relaunch. The path is caller-supplied, so the rejection does
  // not name the file's uid or mode.
  if (st.st_uid != uid || (st.st_mode & (S_IWGRP | S_IWOTH)) != 0) {
    XLOGF(
        ERR,
        "not restarting edenfs: the restart sentinel {} has wrong ownership",
        name);
    return folly::makeUnexpected(SentinelError::Rejected);
  }
  return folly::unit;
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
  // The second, independent disarm signal, and the whole decision: a clean
  // shutdown unlinks the marker, so anything recreated at the name before root
  // looks would otherwise revive a daemon the user stopped on purpose.
  const auto sentinel = openSentinel(loc->dir.fd(), loc->name, uid_);
  if (sentinel.hasValue()) {
    return DisarmState::Armed;
  }
  // Only a name root could not examine leaves the state genuinely unknown, and
  // root must not relaunch on a guess. A name holding anything other than this
  // daemon's marker reads as disarmed.
  return sentinel.error() == SentinelError::Indeterminate
      ? DisarmState::Unknown
      : DisarmState::ShutdownAnnounced;
}

std::optional<RestartSentinel::RelaunchCommand>
RestartSentinel::relaunchCommand() const {
  if (!config_.has_value()) {
    return std::nullopt;
  }
  // An argv the parser accepted as empty would reach execve() with no argv[0].
  if (config_->relaunchArgv.empty()) {
    XLOG(
        ERR,
        "not restarting edenfs: the restart arguments carry no relaunch command");
    return std::nullopt;
  }
  return RelaunchCommand{config_->relaunchArgv, config_->relaunchEnv};
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
