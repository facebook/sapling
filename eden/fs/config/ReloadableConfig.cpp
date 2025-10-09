/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/config/EdenConfig.h"

namespace facebook::eden {

ReloadableConfig::ReloadableConfig(std::shared_ptr<const EdenConfig> config)
    : state_{std::move(config)} {}

ReloadableConfig::~ReloadableConfig() = default;

void ReloadableConfig::maybeReload() {
  auto config = getEdenConfig();
  if (auto newConfig = config->maybeReload()) {
    state_ = newConfig;
    XLOG(INFO, "EdenConfig reloaded");
  }
}

} // namespace facebook::eden
