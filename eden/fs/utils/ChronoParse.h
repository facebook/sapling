/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <chrono>

#include <folly/Expected.h>
#include <folly/Portability.h>
#include <folly/Range.h>

namespace facebook {
namespace eden {

enum class ChronoParseError {
  UnknownUnit,
  InvalidChronoUnitOrder,
  Overflow,
  EmptyInputString,
  InvalidLeadingChar,
  NoDigits,
  NonDigitChar,
  NonWhitespaceAfterEnd,
  OtherError,
};

/**
 * Get a human-readable string describing a ChronoParseError code.
 */
folly::StringPiece chronoParseErrorToString(ChronoParseError error);

/**
 * Append the human-readable description of a ChronoParseError to a string.
 *
 * This allows ChronoParseError arguments to be used with
 * folly::to<std::string>().
 */
template <typename String>
void toAppend(ChronoParseError error, String* result) {
  toAppend(chronoParseErrorToString(error), result);
}

/**
 * Parse a string to a std::chrono::nanoseconds duration.
 */
FOLLY_NODISCARD folly::Expected<std::chrono::nanoseconds, ChronoParseError>
stringToDuration(folly::StringPiece src);

/**
 * Convert a duration value to a string.
 *
 * The resulting string can be parsed with stringToDuration().
 */
std::string durationToString(std::chrono::nanoseconds duration);

} // namespace eden
} // namespace facebook
