/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenStructuredLogger.h"

namespace facebook::eden {

EdenStructuredLogger::EdenStructuredLogger(
    std::shared_ptr<ScribeLogger> scribeLogger,
    SessionInfo sessionInfo)
    : ScubaStructuredLogger{std::move(scribeLogger), std::move(sessionInfo)} {}

DynamicEvent EdenStructuredLogger::populateDefaultFields(
    std::optional<const char*> type) {
  DynamicEvent event = StructuredLogger::populateDefaultFields(type);
  if (sessionInfo_.ciInstanceId.has_value()) {
    event.addInt("sandcastle_instance_id", *sessionInfo_.ciInstanceId);
  }
  event.addString("edenver", sessionInfo_.appVersion);
  event.addString("logged_by", "edenfs");

  if (!sessionInfo_.crossEnvSessionId.empty()) {
    event.addString("ces_id", sessionInfo_.crossEnvSessionId);
  }

  if (!sessionInfo_.systemFingerprint.empty()) {
    event.addString("system_fingerprint", sessionInfo_.systemFingerprint);
  }

  return event;
}

} // namespace facebook::eden
