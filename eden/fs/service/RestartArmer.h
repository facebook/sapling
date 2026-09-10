/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <atomic>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <folly/Synchronized.h>

#include "eden/common/utils/PathFuncs.h"

namespace facebook::eden {

class EdenStateDir;
class PrivHelper;
class ReloadableConfig;

/**
 * The daemon half of privhelper-driven restarts: creates the restart sentinel
 * and gives the privhelper the policy to relaunch this daemon under.
 *
 * Only macOS arms; arm() is a no-op elsewhere. Removing the sentinel is
 * compiled on every platform.
 */
class RestartArmer {
 public:
  /**
   * @param privHelper the privhelper to arm; must outlive this object.
   * @param config read afresh on every arm, so a reload is picked up.
   * @param stateDir holds the daemon args file and names the sentinels; must
   *    outlive this object.
   */
  RestartArmer(
      PrivHelper* privHelper,
      std::shared_ptr<ReloadableConfig> config,
      const EdenStateDir& stateDir);

  /**
   * Send the privhelper the policy to restart this daemon under, and create
   * the sentinel whose existence says "still armed".
   *
   * No-op unless this is macOS with privhelper:restart-edenfs-on-crash set.
   * Best effort: a daemon started without edenfsctl has no recorded command to
   * arm on, and the only consequence is that it will not be restarted.
   */
  void arm();

  /**
   * Remove the sentinel the most recent arm created. Idempotent, and a no-op
   * when nothing has been armed.
   */
  void removeSentinel();

  /**
   * Whether the privhelper accepted our restart configuration. Only ever true
   * on macOS with the feature enabled.
   */
  bool armed() const;

  /**
   * Forget that the privhelper accepted a configuration, so that the next arm
   * answers for itself rather than for the one before it. Leaves the sentinel
   * alone; removeSentinel() is the other half.
   */
  void clearArmed();

 private:
#ifdef __APPLE__
  /** What to relaunch this daemon with, in the shape the restart args carry. */
  struct RelaunchCommand {
    std::vector<std::string> argv;
    std::vector<std::pair<std::string, std::string>> env;
  };

  /**
   * The `argv` and `env` edenfsctl recorded for this daemon, read on the first
   * arm and kept.
   *
   * Returns nullopt, having logged why, if there is nothing to relaunch with.
   */
  std::optional<RelaunchCommand> getRelaunchCommand();
#endif // __APPLE__

  // Only arm() ever talks to the privhelper, so off macOS nothing reads this.
  [[maybe_unused]] PrivHelper* const privHelper_;
  const std::shared_ptr<ReloadableConfig> config_;
  // Likewise, only arm() names a sentinel.
  [[maybe_unused]] const EdenStateDir& stateDir_;
  const AbsolutePath daemonArgsPath_;

  /**
   * The sentinel the most recent arm created, so that a disarm removes the
   * file that arm made rather than a name rebuilt from a fresh token.
   */
  folly::Synchronized<std::optional<AbsolutePath>> sentinelPath_;

  // Shared with the in-flight setRestartArgs continuation, which can outlive
  // this object; that continuation must therefore never capture `this`.
  const std::shared_ptr<std::atomic<bool>> armed_{
      std::make_shared<std::atomic<bool>>(false)};

#ifdef __APPLE__
  /**
   * Memoizes getRelaunchCommand(). The args file has one fixed path per state
   * directory, so a daemon that failed to take over from us has already
   * replaced its contents with its own command by the time we re-arm.
   */
  folly::Synchronized<std::optional<RelaunchCommand>> relaunchCommand_;
#endif // __APPLE__
};

} // namespace facebook::eden
