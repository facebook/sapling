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
  event.addString("edenver", sessionInfo_.appVersion);
  event.addString("logged_by", "edenfs");

  const auto& fbInfo = sessionInfo_.fbInfo;
  for (const auto& info : fbInfo) {
    const auto& key = info.first;
    const auto& value = info.second;
    std::visit(
        [&](const auto& v) {
          using T = std::decay_t<decltype(v)>;
          if constexpr (std::is_same_v<T, std::string>) {
            event.addString(key, v);
          } else if constexpr (std::is_same_v<T, uint64_t>) {
            event.addInt(key, v);
          }
        },
        value);
  }

  return event;
}

} // namespace facebook::eden
