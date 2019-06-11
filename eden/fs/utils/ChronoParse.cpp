/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/ChronoParse.h"

#include <algorithm>
#include <array>
#include <cctype>
#include <cstdint>
#include <string>
#include <type_traits>

#include <folly/Conv.h>
#include <folly/CppAttributes.h>
#include <folly/logging/xlog.h>

#include "eden/fs/utils/ChronoUnit.h"

using facebook::eden::ChronoUnit;
using folly::ConversionCode;
using folly::Expected;
using folly::makeUnexpected;
using folly::StringPiece;

using namespace folly::string_piece_literals;

namespace {
using namespace facebook::eden;

const ChronoUnit* FOLLY_NULLABLE parseUnit(StringPiece* str) {
  const char* begin = str->begin();
  const char* end = str->end();

  // Skip over any leading whitespace
  const char* unitStart =
      std::find_if_not(begin, end, static_cast<int (*)(int)>(std::isspace));
  if (unitStart == end) {
    return nullptr;
  }

  // Find the end of the unit string
  const char* unitEnd = std::find_if(unitStart, end, [](char c) {
    // Stop at whitespace or a numeric character.
    // This check for a numeric character was based off of the check in
    // folly::findFirstNonDigit()
    return std::isspace(c) || (static_cast<unsigned>(c) - '0') < 10;
  });

  str->advance(unitEnd - begin);
  return lookupChronoUnitInfo(StringPiece(unitStart, unitEnd));
}

bool verifyUnitOrder(const ChronoUnit* first, const ChronoUnit* second) {
  // All of the units that we support either have 1 as the numerator or the
  // denominator.  We currently rely on this behavior here, and do not bother
  // handling strange units like 2/3rds seconds.  We XDCHECK below if this is
  // ever violated.  Also check on invalid units that have 0 as the numerator or
  // denominator.
  XDCHECK_NE(first->num, 0);
  XDCHECK_NE(first->den, 0);
  XDCHECK_NE(second->num, 0);
  XDCHECK_NE(second->den, 0);

  if (first->num == 1) {
    // first is seconds or less
    if (second->num > 1) {
      // second is larger than 1 second.  Invalid ordering.
      XDCHECK_EQ(second->den, 1);
      return false;
    }
    return second->den > first->den;
  } else {
    // first is greater than 1 second
    XDCHECK_EQ(first->den, 1);
    if (second->num == 1) {
      // second is seconds or less.  Valid ordering.
      return true;
    }
    return first->num > second->num;
  }
}

template <typename T>
Expected<
    typename std::enable_if<std::is_unsigned<T>::value, T>::type,
    ChronoParseError>
checkedMultiply(T x, T y) {
  auto result = x * y;
  if (x != 0 && y != result / x) {
    return makeUnexpected(ChronoParseError::Overflow);
  }
  return result;
}

template <typename T>
Expected<
    typename std::enable_if<std::is_unsigned<T>::value, T>::type,
    ChronoParseError>
checkedAdd(T x, T y) {
  if (x > (std::numeric_limits<T>::max() - y)) {
    return makeUnexpected(ChronoParseError::Overflow);
  }
  return x + y;
}

ChronoParseError conversionCodeToParseError(ConversionCode code) {
  switch (code) {
    case ConversionCode::EMPTY_INPUT_STRING:
      return ChronoParseError::EmptyInputString;
    case ConversionCode::NO_DIGITS:
      return ChronoParseError::NoDigits;
    case ConversionCode::NON_DIGIT_CHAR:
      return ChronoParseError::NonDigitChar;
    case ConversionCode::INVALID_LEADING_CHAR:
      return ChronoParseError::InvalidLeadingChar;
    case ConversionCode::NON_WHITESPACE_AFTER_END:
      return ChronoParseError::NonWhitespaceAfterEnd;
    case ConversionCode::POSITIVE_OVERFLOW:
    case ConversionCode::NEGATIVE_OVERFLOW:
    case ConversionCode::ARITH_POSITIVE_OVERFLOW:
    case ConversionCode::ARITH_NEGATIVE_OVERFLOW:
      return ChronoParseError::Overflow;
    case ConversionCode::SUCCESS:
    case ConversionCode::BOOL_OVERFLOW:
    case ConversionCode::BOOL_INVALID_VALUE:
    case ConversionCode::STRING_TO_FLOAT_ERROR:
    case ConversionCode::ARITH_LOSS_OF_PRECISION:
    case ConversionCode::NUM_ERROR_CODES:
      return ChronoParseError::OtherError;
      // We intentionally do not have a default case so we will get
      // compiler warnings if a new ConversionCode is added without updating
      // this switch statement.
  }

  return ChronoParseError::OtherError;
}

} // namespace

namespace facebook {
namespace eden {

StringPiece chronoParseErrorToString(ChronoParseError error) {
  switch (error) {
    case ChronoParseError::UnknownUnit:
      return "unknown duration unit specifier"_sp;
    case ChronoParseError::InvalidChronoUnitOrder:
      return "duration units must be listed from largest to smallest"_sp;
    case ChronoParseError::Overflow:
      return "overflow"_sp;
    case ChronoParseError::EmptyInputString:
      return "empty input string"_sp;
    case ChronoParseError::InvalidLeadingChar:
      return "invalid leading character"_sp;
    case ChronoParseError::NoDigits:
      return "no digits found in input string"_sp;
    case ChronoParseError::NonDigitChar:
      return "non-digit character found"_sp;
    case ChronoParseError::NonWhitespaceAfterEnd:
      return "non-whitespace character found after end of input"_sp;
    case ChronoParseError::OtherError:
      return "other error"_sp;
  }

  return "unexpected error"_sp;
}

Expected<std::chrono::nanoseconds, ChronoParseError> stringToDuration(
    StringPiece src) {
  using Duration = std::chrono::nanoseconds;
  using Rep = Duration::rep;
  using UnsignedRep = typename std::make_unsigned<Duration::rep>::type;
  ChronoUnit desiredUnits{
      "desired", Duration::period::num, Duration::period::den};

  // Check for a leading negative sign
  bool negative = false;
  src = ltrimWhitespace(src);
  if (src.empty()) {
    return makeUnexpected(ChronoParseError::EmptyInputString);
  }
  if (src.front() == '-') {
    if (!std::is_signed<Rep>::value) {
      // Bail out now if the desired result type is unsigned
      return makeUnexpected(ChronoParseError::InvalidLeadingChar);
    }
    negative = true;
    src.pop_front();
  }

  // Iterate over each <num><unit> section of the input string.
  UnsignedRep result{};
  const ChronoUnit* prevUnit = nullptr;
  while (true) {
    // Parse a numeric substring
    UnsignedRep num;
    auto newSrc = folly::parseTo<UnsignedRep>(src, num);
    if (newSrc.hasError()) {
      // EMPTY_INPUT_STRING will be returned when we reach the end of the
      // string.  This is fine as long as we have parsed at least one previous
      // <num><unit> section.
      if (newSrc.error() == ConversionCode::EMPTY_INPUT_STRING && prevUnit) {
        break;
      }
      return makeUnexpected(conversionCodeToParseError(newSrc.error()));
    }
    src = newSrc.value();

    // Parse a units substring
    auto* unitInfo = parseUnit(&src);
    if (!unitInfo) {
      return makeUnexpected(ChronoParseError::UnknownUnit);
    }

    // Require that the new units are strictly smaller than the previous unit.
    // e.g.,  allow strings like "1m30s" but not "30s1m" or "30s45s"
    if (prevUnit && !verifyUnitOrder(prevUnit, unitInfo)) {
      return makeUnexpected(ChronoParseError::InvalidChronoUnitOrder);
    }
    prevUnit = unitInfo;

    // Update result, checking for overflow.
    auto newResult =
        checkedMultiply(
            num, static_cast<UnsignedRep>(unitInfo->num * desiredUnits.den))
            .then([&](UnsignedRep value) {
              auto valueInDesiredUnits =
                  value / (unitInfo->den * desiredUnits.num);
              return checkedAdd(result, valueInDesiredUnits);
            });
    if (newResult.hasError()) {
      return makeUnexpected(newResult.error());
    }
    result = newResult.value();
  }

  // Convert the result from UnsignedRep to Rep, checking for overflow.
  auto finalResult = folly::tryTo<Rep>(result);
  if (finalResult.hasError()) {
    return makeUnexpected(conversionCodeToParseError(finalResult.error()));
  }
  if (negative) {
    return Duration{-finalResult.value()};
  } else {
    return Duration{finalResult.value()};
  }
}

std::string durationToString(std::chrono::nanoseconds duration) {
  struct SuffixInfo {
    StringPiece suffix;
    uintmax_t nanoseconds;
  };
  constexpr std::array<SuffixInfo, 6> suffixes{
      // We currently use days as the maximum unit when converting to strings.
      // Years and months seem slightly ambiguous: the definition settled on by
      // C++20 isn't necessarily an obvious definition.  Weeks are unambiguous,
      // but it still seems reasonable to use days as our max unit here.
      SuffixInfo{"d", 24 * 60 * 60 * 1'000'000'000ULL},
      SuffixInfo{"h", 60 * 60 * 1'000'000'000ULL},
      SuffixInfo{"m", 60 * 1'000'000'000ULL},
      SuffixInfo{"s", 1'000'000'000},
      SuffixInfo{"ms", 1'000'000},
      SuffixInfo{"us", 1'000},
  };

  if (duration.count() == 0) {
    return "0ns";
  }

  std::string result;
  uintmax_t value;
  if (duration.count() < 0) {
    result.push_back('-');
    // Casting to unsigned before applying negation avoids potentially undefined
    // overflow behavior when processing the smallest possible negative number.
    // Converting a negative signed number to unsigned is well-defined and does
    // what we want, as does applying negation to an unsigned number.
    value = -static_cast<uintmax_t>(duration.count());
  } else {
    value = duration.count();
  }

  for (const auto& suffix : suffixes) {
    if (value > suffix.nanoseconds) {
      auto count = value / suffix.nanoseconds;
      value = value % suffix.nanoseconds;
      folly::toAppend(count, suffix.suffix, &result);
    }
  }
  if (value > 0) {
    folly::toAppend(value, "ns", &result);
  }

  return result;
}

} // namespace eden
} // namespace facebook
