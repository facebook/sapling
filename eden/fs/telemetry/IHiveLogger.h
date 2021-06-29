/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/telemetry/SessionInfo.h"

namespace facebook::eden {

// TODO: Deprecate ScribeLogger and rename this class ScribeLogger.
class IHiveLogger {
 public:
  explicit IHiveLogger(SessionInfo sessionInfo)
      : sessionInfo_{std::move(sessionInfo)} {}
  virtual ~IHiveLogger() = default;

 protected:
  SessionInfo sessionInfo_;
};

class NullHiveLogger : public IHiveLogger {
 public:
  NullHiveLogger() : IHiveLogger{SessionInfo{}} {}
};

} // namespace facebook::eden
