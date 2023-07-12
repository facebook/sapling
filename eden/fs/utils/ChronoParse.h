/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>

#include <folly/Expected.h>
#include <folly/Portability.h>
#include <folly/Range.h>

namespace facebook::eden {

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
std::string_view chronoParseErrorToString(ChronoParseError error);

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

} // namespace facebook::eden
