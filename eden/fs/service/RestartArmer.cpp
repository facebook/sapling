/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/RestartArmer.h"

#ifndef _WIN32
#include <dirent.h>
#endif

#include <algorithm>
#include <limits>
#include <memory>
#include <string>
#include <string_view>
#include <utility>

#ifdef __APPLE__
#include <chrono>
#endif

#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Random.h>
#include <folly/String.h>
#include <folly/futures/Future.h>
#include <folly/json/json.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Fcntl.h>
#include <folly/portability/SysStat.h>
#include <folly/portability/Unistd.h>

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/privhelper/PrivHelper.h"
#include "eden/fs/service/EdenStateDir.h"

#ifdef __APPLE__
#include "eden/fs/config/EdenConfig.h"
#endif

namespace facebook::eden {

namespace {
void unlinkSentinel(const AbsolutePath& sentinel) {
  // A bare unlink() on purpose: one synchronous syscall that touches no event
  // loop, so it still disarms when the loop is too wedged to deliver the
  // clean-shutdown notification.
  if (::unlink(sentinel.c_str()) != 0) {
    // Captured before anything else can run: formatting the path below is not
    // guaranteed to leave errno alone.
    const int err = errno;
    if (err != ENOENT) {
      XLOGF(WARN, "failed to unlink {}: {}", sentinel, folly::errnoStr(err));
    }
  }
}

#ifdef __APPLE__
/**
 * Create the empty sentinel, and report whether it is now there.
 *
 * O_EXCL: the sentinel has to be a file this call made, so that neither its
 * mode nor its emptiness can belong to something already at that name.
 */
bool createSentinel(const AbsolutePath& sentinel) {
  const int fd = folly::openNoInt(
      sentinel.c_str(), O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
  if (fd < 0) {
    const int err = errno;
    XLOGF(
        WARN,
        "failed to create {}: {}; edenfs will not be auto-restarted",
        sentinel,
        folly::errnoStr(err));
    return false;
  }
  const folly::File file{fd, /*ownsFd=*/true};

  // open()'s mode is only whatever the umask leaves of it, so pin 0600 here.
  if (::fchmod(fd, 0600) != 0) {
    const int err = errno;
    XLOGF(
        WARN,
        "failed to set the mode of {}: {}; edenfs will not be auto-restarted",
        sentinel,
        folly::errnoStr(err));
    unlinkSentinel(sentinel);
    return false;
  }
  return true;
}

bool isDecimalDigit(char c) {
  return c >= '0' && c <= '9';
}

bool isLowerHexDigit(char c) {
  return isDecimalDigit(c) || (c >= 'a' && c <= 'f');
}

/**
 * Whether `name` is a per-generation sentinel: `namePrefix`, a decimal pid, a
 * dot, and the sixteen lowercase hex digits of the token.
 *
 * Exact rather than a prefix test, because other names in the state directory
 * share the prefix and are not ours to unlink.
 */
bool isSentinelName(std::string_view name, std::string_view namePrefix) {
  if (!name.starts_with(namePrefix)) {
    return false;
  }
  const auto rest = name.substr(namePrefix.size());
  const auto dot = rest.find('.');
  if (dot == std::string_view::npos || dot == 0) {
    return false;
  }
  const auto pid = rest.substr(0, dot);
  const auto token = rest.substr(dot + 1);
  return token.size() == 16 &&
      std::all_of(pid.begin(), pid.end(), isDecimalDigit) &&
      std::all_of(token.begin(), token.end(), isLowerHexDigit);
}

/**
 * Remove the sentinels earlier daemon generations left in `stateDirPath`,
 * keeping the ones this pid armed under.
 *
 * A sentinel that cannot be removed is logged and skipped.
 *
 * A live pid is reaped like any other: whoever holds the state directory lock
 * is the only daemon entitled to be armed against it.
 */
void reapOtherGenerationSentinels(
    const std::string& stateDirPath,
    std::string_view namePrefix) {
  const std::unique_ptr<DIR, int (*)(DIR*)> dir{
      ::opendir(stateDirPath.c_str()), &::closedir};
  if (dir == nullptr) {
    const int err = errno;
    XLOGF(
        WARN,
        "failed to open {} to reap old restart sentinels: {}",
        stateDirPath,
        folly::errnoStr(err));
    return;
  }

  // Unlinked relative to the handle we listed, rather than by path, so the
  // name resolves in the directory we scanned and a sentinel that turns out to
  // be a symlink is removed rather than followed.
  const int dirFd = ::dirfd(dir.get());
  const auto ownPrefix =
      std::string{namePrefix} + std::to_string(::getpid()) + '.';

  while (true) {
    errno = 0;
    const struct dirent* const entry = ::readdir(dir.get());
    if (entry == nullptr) {
      if (const int err = errno; err != 0) {
        XLOGF(
            WARN,
            "failed to list {} while reaping old restart sentinels: {}",
            stateDirPath,
            folly::errnoStr(err));
      }
      break;
    }

    const std::string_view name{entry->d_name};
    if (!isSentinelName(name, namePrefix) || name.starts_with(ownPrefix)) {
      continue;
    }
    if (::unlinkat(dirFd, entry->d_name, 0) != 0) {
      const int err = errno;
      if (err != ENOENT) {
        XLOGF(
            WARN,
            "failed to reap the restart sentinel {} in {}: {}",
            name,
            stateDirPath,
            folly::errnoStr(err));
      }
    }
  }
}
#endif // __APPLE__
} // namespace

RestartArmer::RestartArmer(
    PrivHelper* privHelper,
    std::shared_ptr<ReloadableConfig> config,
    const EdenStateDir& stateDir)
    : privHelper_{privHelper},
      config_{std::move(config)},
      stateDir_{stateDir},
      daemonArgsPath_{stateDir.getDaemonArgsPath()} {}

#ifdef __APPLE__
std::optional<folly::dynamic> RestartArmer::getRelaunchCommand() {
  auto cached = relaunchCommand_.wlock();
  if (cached->has_value()) {
    return *cached;
  }

  const auto& argsPath = daemonArgsPath_;
  folly::dynamic relaunchArgv = folly::dynamic::array;
  folly::dynamic relaunchEnv = folly::dynamic::object;
  try {
    std::string contents;
    if (!folly::readFile(argsPath.c_str(), contents)) {
      XLOGF(
          DBG2,
          "no daemon args file at {}; edenfs will not be auto-restarted",
          argsPath);
      return std::nullopt;
    }

    const auto parsed = folly::parseJson(contents);
    const auto* restartCmd = parsed.get_ptr("restart_cmd");
    if (!restartCmd || !restartCmd->isArray() || restartCmd->empty()) {
      XLOGF(
          WARN,
          "daemon args file {} has no usable restart_cmd; edenfs will not be auto-restarted",
          argsPath);
      return std::nullopt;
    }
    for (const auto& arg : *restartCmd) {
      relaunchArgv.push_back(arg.asString());
    }
    if (const auto* env = parsed.get_ptr("env"); env && env->isObject()) {
      for (const auto& [key, value] : env->items()) {
        relaunchEnv[key.asString()] = value.asString();
      }
    }
  } catch (const std::exception& ex) {
    XLOGF(
        WARN,
        "failed to read daemon args file {}: {}",
        argsPath,
        folly::exceptionStr(ex));
    return std::nullopt;
  }

  if (relaunchEnv.empty()) {
    // The privhelper replaces the child's environment wholesale, so relaunching
    // with nothing would give the new daemon no PATH, HOME or USER.
    XLOGF(
        WARN,
        "daemon args file {} has an empty environment; edenfs will not be auto-restarted",
        argsPath);
    return std::nullopt;
  }

  *cached = folly::dynamic::object("argv", std::move(relaunchArgv))(
      "env", std::move(relaunchEnv));
  return *cached;
}
#endif // __APPLE__

void RestartArmer::arm() {
#ifdef __APPLE__
  const auto config = config_->getEdenConfig();
  if (!config->restartEdenfsOnCrash.getValue()) {
    return;
  }

  if (!getRelaunchCommand().has_value()) {
    return;
  }

  EdenFsRestartArgs args;
  args.enabled = true;
  // Carry forward the budget spent by the privhelper that spawned us, so that
  // a daemon crashing in a loop is stopped rather than restarted for ever.
  args.restartCount = static_cast<uint32_t>(std::min<uint64_t>(
      readEdenFsRestartCounterEnv(kEdenFsRestartCountEnv),
      std::numeric_limits<uint32_t>::max()));
  args.firstRestartEpochSec =
      readEdenFsRestartCounterEnv(kEdenFsFirstRestartAtEnv);
  args.maxRestarts = config->restartEdenfsMaxCount.getValue();
  args.windowSeconds = std::max(
      uint32_t{1},
      static_cast<uint32_t>(std::chrono::duration_cast<std::chrono::seconds>(
                                config->restartEdenfsWindow.getValue())
                                .count()));

  // A token drawn afresh on every arm: a daemon re-arming after a failed
  // takeover keeps its pid, and under a name the earlier arm could reproduce,
  // that arm's late failure continuation would delete this one's marker.
  const auto sentinel =
      stateDir_.getRestartSentinelPath(::getpid(), folly::Random::rand64());
  args.sentinelPath = sentinel.asString();
  // Never 0: that value is reserved for the absent nonce a sentinel from an
  // older daemon reads as. Bounded to 63 bits so it survives folly::dynamic's
  // signed integer as a positive number.
  args.sentinelNonce = folly::Random::rand64(1, uint64_t{1} << 63);
  {
    auto sentinelPath = sentinelPath_.wlock();
    // Created before the request, so the privhelper never sees a missing path
    // and concludes that we already disarmed.
    if (!createSentinel(sentinel)) {
      return;
    }
    const auto previous = std::exchange(*sentinelPath, sentinel);
    if (previous.has_value()) {
      unlinkSentinel(*previous);
    }
  }

  // Here, and not sooner or later: a daemon that fails before its own marker
  // exists must leave earlier ones for their privhelpers to find, and a sweep
  // deferred to the reply below could outrun a later generation's arm.
  reapOtherGenerationSentinels(
      stateDir_.getPath().asString(), stateDir_.getRestartSentinelNamePrefix());

  // Cannot be waited on: the reply is driven by the main EventBase, the thread
  // we are on. The continuation carries no executor, so it runs inline on
  // whoever completes the request: that EventBase, or shutdownPrivhelper().
  folly::futures::detachOnGlobalCPUExecutor(
      privHelper_->setRestartArgs(args)
          .thenTry(
              [sentinel, armed = armed_](folly::Try<folly::Unit>&& result) {
                if (result.hasException()) {
                  // Most likely a privhelper too old to know the request. Leave
                  // ourselves disarmed.
                  XLOGF(
                      WARN,
                      "failed to send restart args to the privhelper: {}",
                      result.exception().what());
                  unlinkSentinel(sentinel);
                  return;
                }
                armed->store(true, std::memory_order_release);
              })
          .semi());
#endif // __APPLE__
}

void RestartArmer::removeSentinel() {
  auto sentinelPath = sentinelPath_.wlock();
  const auto sentinel = std::exchange(*sentinelPath, std::nullopt);
  if (sentinel.has_value()) {
    unlinkSentinel(*sentinel);
  }
}

bool RestartArmer::armed() const {
  return armed_->load(std::memory_order_acquire);
}

} // namespace facebook::eden
