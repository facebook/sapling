/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/RestartArmer.h"

#include <algorithm>
#include <limits>
#include <utility>

#ifdef __APPLE__
#include <chrono>
#endif

#include <folly/FileUtil.h>
#include <folly/Random.h>
#include <folly/String.h>
#include <folly/futures/Future.h>
#include <folly/json/json.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/privhelper/PrivHelper.h"

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
} // namespace

RestartArmer::RestartArmer(
    PrivHelper* privHelper,
    std::shared_ptr<ReloadableConfig> config,
    AbsolutePath daemonArgsPath,
    AbsolutePath sentinelPath)
    : privHelper_{privHelper},
      config_{std::move(config)},
      daemonArgsPath_{std::move(daemonArgsPath)},
      sentinelPath_{std::move(sentinelPath)} {}

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

  const auto relaunch = getRelaunchCommand();
  if (!relaunch.has_value()) {
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

  const auto& sentinel = sentinelPath_;
  args.sentinelPath = sentinel.asString();
  // Never 0: that value is reserved for the absent nonce a sentinel from an
  // older daemon reads as. Bounded to 63 bits so it survives folly::dynamic's
  // signed integer as a positive number.
  args.sentinelNonce = folly::Random::rand64(1, uint64_t{1} << 63);
  // The sentinel carries the relaunch command as well as arming the privhelper,
  // so the RPC never has to. Written before the request, so the privhelper
  // never sees a missing path and concludes that we already disarmed.
  const folly::dynamic command = folly::dynamic::object(
      "argv", relaunch->at("argv"))("env", relaunch->at("env"))(
      "nonce", static_cast<int64_t>(args.sentinelNonce));
  // Atomically, onto a file created for this write: the root privhelper refuses
  // a sentinel that anyone but us can write, and a mode passed to open() would
  // leave whatever an earlier generation left behind in place.
  if (const int rc = folly::writeFileAtomicNoThrow(
          sentinel.asString(),
          folly::toJson(command),
          folly::WriteFileAtomicOptions().setPermissions(0600));
      rc != 0) {
    XLOGF(
        WARN,
        "failed to create {}: {}; edenfs will not be auto-restarted",
        sentinel,
        folly::errnoStr(rc));
    return;
  }

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
  unlinkSentinel(sentinelPath_);
}

bool RestartArmer::armed() const {
  return armed_->load(std::memory_order_acquire);
}

} // namespace facebook::eden
