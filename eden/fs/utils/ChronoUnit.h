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

#include <cstdint>

#include <folly/CppAttributes.h>
#include <folly/Range.h>

namespace facebook {
namespace eden {

/**
 * A structure representing information about a duration unit.
 */
struct ChronoUnit {
  folly::StringPiece name;
  intmax_t num;
  intmax_t den;
};

/**
 * Parse a string as a time duration unit.
 *
 * This is used to help parse time strings like "1m30s"
 * Given the unit portion of this string (e.g., "m", "ns") this returns a
 * pointer to an appropriate ChronoUnit, or nullptr if the string does not
 * correspond to a valid unit name.
 *
 * e.g.,
 *   ns --> num=1, den=1000000000
 *   ms --> num=1, den=1000
 *   day --> num=86400, den=1
 */
const ChronoUnit* FOLLY_NULLABLE
lookupChronoUnitInfo(folly::StringPiece unitName);

} // namespace eden
} // namespace facebook
