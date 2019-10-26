/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/LogEvent.h"

#include <folly/Conv.h>
#include <folly/Unicode.h>
#include <folly/logging/xlog.h>

namespace {
void validateUtf8(folly::StringPiece sp) {
  auto* p = reinterpret_cast<const unsigned char*>(sp.begin());
  auto* const end = reinterpret_cast<const unsigned char*>(sp.end());
  while (p < end) {
    (void)folly::utf8ToCodePoint(p, end, false);
  }
}
} // namespace

namespace facebook {
namespace eden {

void DynamicEvent::addInt(std::string name, int64_t value) {
  auto [iter, inserted] = ints_.emplace(std::move(name), value);
  if (!inserted) {
    throw std::logic_error(folly::to<std::string>(
        "Attempted to insert duplicate int: ", iter->first));
  }
}

void DynamicEvent::addString(std::string name, std::string value) {
  validateUtf8(value);
  auto [iter, inserted] = strings_.emplace(std::move(name), std::move(value));
  if (!inserted) {
    throw std::logic_error(folly::to<std::string>(
        "Attempted to insert duplicate string: ", iter->first));
  }
}

void DynamicEvent::addDouble(std::string name, double value) {
  XCHECK(std::isfinite(value))
      << "Attempted to insert double-precision value that cannot be represented in JSON: "
      << name;
  auto [iter, inserted] = doubles_.emplace(std::move(name), value);
  if (!inserted) {
    throw std::logic_error(folly::to<std::string>(
        "Attempted to insert duplicate double: ", iter->first));
  }
}

} // namespace eden
} // namespace facebook
