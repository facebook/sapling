/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/telemetry/ScribeLogger.h"
#include "eden/common/telemetry/ScubaStructuredLogger.h"
#include "eden/common/telemetry/SessionInfo.h"

namespace facebook::eden {

class EdenStructuredLogger : public ScubaStructuredLogger {
 public:
  explicit EdenStructuredLogger(
      std::shared_ptr<ScribeLogger> scribeLogger,
      SessionInfo sessionInfo);
  virtual ~EdenStructuredLogger() override = default;

 protected:
  virtual DynamicEvent populateDefaultFields(const char* type) override;
};

} // namespace facebook::eden
