/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/telemetry/ScribeLogger.h"
#include "eden/common/telemetry/SessionInfo.h"
#include "eden/fs/telemetry/EdenStructuredLogger.h"

namespace facebook::eden {

class FileAccessStructuredLogger : public EdenStructuredLogger {
 public:
  explicit FileAccessStructuredLogger(
      std::shared_ptr<ScribeLogger> scribeLogger,
      SessionInfo sessionInfo);
  virtual ~FileAccessStructuredLogger() override = default;

 protected:
  virtual DynamicEvent populateDefaultFields(
      std::optional<const char*> type) override;
};

} // namespace facebook::eden
