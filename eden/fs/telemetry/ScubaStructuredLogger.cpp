/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ScubaStructuredLogger.h"

#include <folly/json.h>
#include <folly/logging/xlog.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/telemetry/SubprocessScribeLogger.h"

namespace facebook {
namespace eden {

namespace {

template <typename Key, typename Value>
folly::dynamic dynamicMap(const std::unordered_map<Key, Value>& map) {
  folly::dynamic o = folly::dynamic::object;
  for (const auto& [key, value] : map) {
    o[key] = value;
  }
  return o;
}

} // namespace

ScubaStructuredLogger::ScubaStructuredLogger(
    std::shared_ptr<ScribeLogger> scribeLogger,
    SessionInfo sessionInfo)
    : StructuredLogger{true, std::move(sessionInfo)},
      scribeLogger_{std::move(scribeLogger)} {}

void ScubaStructuredLogger::logDynamicEvent(DynamicEvent event) {
  folly::dynamic document = folly::dynamic::object;

  const auto& intMap = event.getIntMap();
  if (!intMap.empty()) {
    document["int"] = dynamicMap(intMap);
  }

  const auto& stringMap = event.getStringMap();
  if (!stringMap.empty()) {
    document["normal"] = dynamicMap(stringMap);
  }

  const auto& doubleMap = event.getDoubleMap();
  if (!doubleMap.empty()) {
    document["double"] = dynamicMap(doubleMap);
  }

  scribeLogger_->log(folly::toJson(document));
}

} // namespace eden
} // namespace facebook
