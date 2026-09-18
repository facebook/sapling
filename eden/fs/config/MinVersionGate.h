/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <optional>
#include <string_view>
#include <tuple>

namespace facebook::eden {

/**
 * An EdenFS package version as printed by `eden version`, e.g.
 * `20260915-081814`.
 */
struct EdenVersion {
  uint32_t date{0}; // yyyymmdd
  uint32_t time{0}; // hhmmss, 0 when the version carries no time part

  friend bool operator==(const EdenVersion& a, const EdenVersion& b) {
    return std::tie(a.date, a.time) == std::tie(b.date, b.time);
  }
  friend bool operator<(const EdenVersion& a, const EdenVersion& b) {
    return std::tie(a.date, a.time) < std::tie(b.date, b.time);
  }
};

/**
 * Parses `yyyymmdd[-hhmmss]`. Anything after the leading date and time digits
 * is ignored. Returns nullopt when the string does not start with a date.
 */
std::optional<EdenVersion> parseEdenVersion(std::string_view version);

/**
 * Version of this EdenFS build. Nullopt for a development build, which
 * carries no package version and is treated as newer than any release. A
 * release whose version cannot be parsed is reported as version 0, so every
 * gate fails closed for it.
 */
std::optional<EdenVersion> getBuildEdenVersion();

/**
 * A config key of the form `name@min-version=yyyymmdd[-hhmmss]`. The config
 * manager writes such keys for rollouts it could evaluate except for the
 * version of the EdenFS that will read them; the reader applies the entry
 * under `name` only when its own version is at least `minVersion`.
 */
struct MinVersionGatedKey {
  std::string_view name;
  std::string_view minVersion;
};

/**
 * Splits a gated key. Returns nullopt when `key` carries no gate.
 */
std::optional<MinVersionGatedKey> parseMinVersionGatedKey(std::string_view key);

} // namespace facebook::eden
