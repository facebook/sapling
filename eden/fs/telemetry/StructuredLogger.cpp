/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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

namespace facebook {
namespace eden {

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
  event.addString("os", sessionInfo_.os);
  event.addString("osver", sessionInfo_.osVersion);
  event.addString("edenver", sessionInfo_.edenVersion);
  return event;
}

} // namespace eden
} // namespace facebook
