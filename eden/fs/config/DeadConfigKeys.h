/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string_view>

namespace facebook::eden {

/**
 * Config keys ("section:key") that EdenFS no longer reads, but that config
 * sources (in particular remotely-deployed dynamic configs) may still set.
 * Keys listed here are silently ignored instead of triggering the
 * "Ignoring unknown key in eden config" warning.
 *
 * Add a key here when deleting its ConfigSetting from EdenConfig. Remove it
 * once no config source sets the key anymore.
 */
inline constexpr std::string_view kDeadConfigKeys[] = {
    "coroutines:enable-phase1",
    "coroutines:enable-phase2",
    "coroutines:enable-phase3",
    "coroutines:enable-phase5",
    "coroutines:enable-phase6",
    "coroutines:enable-phase11",
    "experimental:batch-checkout-dir-mutations",
    "experimental:filteredfs-optimize-unfiltered",
    "experimental:glob-skip-redundant-origin-hashes",
    "experimental:ignore-prefetch-result",
    "experimental:prefetch-optimizations-v2",
    "experimental:skip-checkout-child-overlay-writes",
    "overlay:direct-serialization",
    "telemetry:enable-xplatlogger-events",
};

inline bool isDeadConfigKey(std::string_view section, std::string_view key) {
  for (auto dead : kDeadConfigKeys) {
    if (dead.size() == section.size() + 1 + key.size() &&
        dead.starts_with(section) && dead[section.size()] == ':' &&
        dead.ends_with(key)) {
      return true;
    }
  }
  return false;
}

} // namespace facebook::eden
