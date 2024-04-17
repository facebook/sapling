/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

namespace facebook::eden {

class SaplingBackingStoreOptions {
 public:
  /* implicit */ SaplingBackingStoreOptions(
      std::optional<bool> ignoreFilteredPathsConfig)
      : ignoreFilteredPathsConfig{ignoreFilteredPathsConfig} {}

  bool ignoreConfigFilter() {
    return ignoreFilteredPathsConfig.value_or(false);
  }

  std::optional<bool> ignoreFilteredPathsConfig;
};

} // namespace facebook::eden
