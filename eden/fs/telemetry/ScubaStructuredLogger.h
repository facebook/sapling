/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include "eden/fs/telemetry/StructuredLogger.h"

namespace facebook {
namespace eden {

class EdenConfig;
class ScribeLogger;

class ScubaStructuredLogger final : public StructuredLogger {
 public:
  ScubaStructuredLogger(
      std::shared_ptr<ScribeLogger> scribeLogger,
      SessionInfo sessionInfo);

 private:
  void logDynamicEvent(DynamicEvent event) override;

  std::shared_ptr<ScribeLogger> scribeLogger_;
};

} // namespace eden
} // namespace facebook
