/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/StructuredLogger.h"

#include "eden/fs/telemetry/SessionId.h"

#include <time.h>

namespace {
/**
 * The log database populates the time field automatically.
 */
constexpr bool kExplicitTimeField = true;
} // namespace

namespace facebook::eden {

StructuredLogger::StructuredLogger(bool enabled, SessionInfo sessionInfo)
    : enabled_{enabled},
      sessionId_{getSessionId()},
      sessionInfo_{std::move(sessionInfo)} {}

DynamicEvent StructuredLogger::populateDefaultFields(const char* type) {
  DynamicEvent event;
  if (kExplicitTimeField) {
    event.addInt("time", ::time(nullptr));
  }
  event.addInt("session_id", sessionId_);
  event.addString("type", type);
  event.addString("user", sessionInfo_.username);
  event.addString("host", sessionInfo_.hostname);
  if (sessionInfo_.sandcastleInstanceId.has_value()) {
    event.addInt("sandcastle_instance_id", *sessionInfo_.sandcastleInstanceId);
  }
  event.addString("os", sessionInfo_.os);
  event.addString("osver", sessionInfo_.osVersion);
  event.addString("edenver", sessionInfo_.edenVersion);
#if defined(__APPLE__)
  event.addString("system_architecture", sessionInfo_.systemArchitecture);
#endif
  return event;
}

} // namespace facebook::eden
