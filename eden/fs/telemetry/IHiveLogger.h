/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/SessionInfo.h"

namespace facebook::eden {

class EdenConfig;
class EdenServer;

// TODO: Deprecate ScribeLogger and rename this class ScribeLogger.
class IHiveLogger {
 public:
  IHiveLogger(
      SessionInfo sessionInfo,
      std::shared_ptr<const EdenConfig> edenConfig,
      EdenServer* edenServer)
      : sessionInfo_{std::move(sessionInfo)},
        reloadableConfig_{edenConfig},
        edenServer_{edenServer} {}
  virtual ~IHiveLogger() = default;

  /**
   * This allows us to create objects derived from IHiveLogger with
   * only a IHiveLogger pointer
   */
  virtual std::unique_ptr<IHiveLogger> create() = 0;

 protected:
  SessionInfo sessionInfo_;
  ReloadableConfig reloadableConfig_;
  EdenServer* edenServer_;
};

class NullHiveLogger : public IHiveLogger {
 public:
  NullHiveLogger() : IHiveLogger{SessionInfo{}, {}, nullptr} {}

  std::unique_ptr<IHiveLogger> create() override {
    return std::make_unique<NullHiveLogger>();
  }
};

} // namespace facebook::eden
