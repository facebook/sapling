/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <string>

namespace facebook {
namespace eden {

/**
 * An interface to a scribe logger implementation.
 *
 * Subclasses must override either of the log overloads.
 *
 * Messages must not contain newlines. Messages are not durable. They may be
 * dropped under load or for other reasons.
 */
class ScribeLogger {
 public:
  virtual ~ScribeLogger() = default;
  virtual void log(folly::StringPiece message) {
    return log(message.str());
  }
  virtual void log(std::string message) {
    return log(folly::StringPiece{message});
  }
};

} // namespace eden
} // namespace facebook
