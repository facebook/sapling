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

#include <folly/Synchronized.h>
#include <folly/json/dynamic.h>

#include "eden/common/utils/PathFuncs.h"

namespace facebook::eden {

class PrivHelper;
class ReloadableConfig;

/**
 * The daemon half of privhelper-driven restarts: writes the restart sentinel
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
   * @param daemonArgsPath where edenfsctl recorded this daemon's command.
   * @param sentinelPath the restart sentinel to write and remove.
   */
  RestartArmer(
      PrivHelper* privHelper,
      std::shared_ptr<ReloadableConfig> config,
      AbsolutePath daemonArgsPath,
      AbsolutePath sentinelPath);

  /**
   * Give the privhelper what it needs to relaunch this daemon after a crash,
   * and create the sentinel whose existence says "still armed".
   *
   * No-op unless this is macOS with privhelper:restart-edenfs-on-crash set.
   * Best effort: a daemon started without edenfsctl has no recorded command,
   * and the only consequence is that it will not be restarted.
   */
  void arm();

  /** Remove this daemon's restart sentinel. Idempotent. */
  void removeSentinel();

  /**
   * Whether the privhelper accepted our restart configuration. Only ever true
   * on macOS with the feature enabled.
   */
  bool armed() const;

 private:
#ifdef __APPLE__
  /**
   * The `argv` and `env` edenfsctl recorded for this daemon, read on the first
   * arm and kept.
   *
   * Returns nullopt, having logged why, if there is nothing to relaunch with.
   */
  std::optional<folly::dynamic> getRelaunchCommand();
#endif // __APPLE__

  // Only arm() ever talks to the privhelper, so off macOS nothing reads this.
  [[maybe_unused]] PrivHelper* const privHelper_;
  const std::shared_ptr<ReloadableConfig> config_;
  const AbsolutePath daemonArgsPath_;
  const AbsolutePath sentinelPath_;

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
  folly::Synchronized<std::optional<folly::dynamic>> relaunchCommand_;
#endif // __APPLE__
};

} // namespace facebook::eden
