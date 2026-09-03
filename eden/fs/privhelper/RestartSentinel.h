/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifdef __APPLE__

#include <folly/File.h>
#include <sys/types.h>
#include <cstdint>
#include <optional>
#include <string>
#include <utility>
#include <vector>
#include "eden/fs/privhelper/PrivHelper.h"

namespace facebook::eden {

/**
 * Tracks whether the privhelper should relaunch edenfs, and what to relaunch.
 *
 * The sentinel path arrives over IPC from the unprivileged daemon while this
 * process is root, so every accessor treats it as untrusted input. The
 * directory is resolved once and pinned as an fd; afterwards only the leaf is
 * looked up inside it, and the opened file -- not its name -- is what gets
 * validated.
 */
class RestartSentinel {
 public:
  /**
   * The command to relaunch edenfs with, as read out of the restart sentinel.
   * argv is already stripped of sudo, `--takeover` and inherited file
   * descriptor arguments by the daemon that wrote it.
   */
  struct RelaunchCommand {
    std::vector<std::string> argv;
    std::vector<std::pair<std::string, std::string>> env;
  };

  /** How the two disarm signals read, or that neither could be read. */
  enum class DisarmState {
    /** edenfs neither announced a shutdown nor removed its sentinel. */
    Armed,
    /** edenfs signalled, either way, that it meant to shut down. */
    ShutdownAnnounced,
    /** The sentinel's state could not be determined; root must not guess. */
    Unknown,
  };

  /** @param uid the daemon's user, which has to own the sentinel. */
  explicit RestartSentinel(uid_t uid) : uid_{uid} {}

  /**
   * The only way to replace the restart configuration. Fresh arguments also
   * re-arm: a resolution cached from the previous configuration would stay
   * pinned to its directory, and a clean-shutdown flag would outlive it.
   */
  void setConfig(EdenFsRestartArgs args);

  /** Record that edenfs announced a deliberate shutdown. */
  void noteCleanShutdown();

  /** Whether a configuration arrived and it turns restarts on. */
  bool enabled() const;

  /** The disarm signals, or nullopt when no restart args ever armed them. */
  std::optional<DisarmState> disarmState() const;

  /**
   * Parse the relaunch command out of the restart sentinel, or nullopt if it
   * cannot be read or does not hold one. Only ever called with privileges.
   */
  std::optional<RelaunchCommand> readRelaunchCommand() const;

  /**
   * Applies the circuit breaker. Returns false when the limit is reached.
   * Requires a configuration to have arrived.
   *
   * @param now seconds since the epoch.
   */
  bool admitRestartAttempt(uint64_t now);

  /** The restart budget as it stands after the last admitted attempt. */
  uint32_t restartCount() const;
  uint64_t firstRestartEpochSec() const;

 private:
  /** The directory holding the restart sentinel, and its name within it. */
  struct Location {
    folly::File dir;
    std::string name;
  };

  /**
   * Where the restart sentinel lives, or nullptr when no restart configuration
   * has arrived or the configured path cannot be resolved. The path comes from
   * the unprivileged daemon, so root walks it once and afterwards only looks
   * the leaf up in the pinned directory, which no ancestor rename can redirect.
   */
  const Location* location() const;

  // The daemon's uid, as supplied to PrivHelperServer::init().
  uid_t uid_;

  // Restart configuration most recently supplied by the daemon, if any. Absent
  // means the daemon never finished starting up, so there is nothing to
  // restart and no boot-crash loop is possible.
  std::optional<EdenFsRestartArgs> config_;

  // Whether the daemon announced that its shutdown was deliberate.
  bool cleanShutdownNotified_{false};

  // Resolved on first use, and dropped whenever fresh restart args arrive.
  mutable std::optional<Location> location_;

  // errno of the last resolution failure reported since the last re-arm, 0 for
  // the malformed-path failure, which has none. A failure is retried rather
  // than cached, so that a transient one cannot leave root permanently unable
  // to restart edenfs; a retry that fails the same way again does not log.
  mutable std::optional<int> lastResolutionError_;
};

} // namespace facebook::eden

#endif // __APPLE__
