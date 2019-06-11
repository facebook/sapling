/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/ChronoUnit.h"

#include <folly/portability/GTest.h>

using folly::StringPiece;
using std::make_pair;

namespace {
std::pair<intmax_t, intmax_t> doLookup(StringPiece name) {
  auto* info = facebook::eden::lookupChronoUnitInfo(name);
  if (!info) {
    return {0, 0};
  }
  return {info->num, info->den};
}

std::pair<intmax_t, intmax_t> subsecond(intmax_t den) {
  return {1, den};
}

std::pair<intmax_t, intmax_t> multisecond(intmax_t num) {
  return {num, 1};
}
} // namespace

TEST(ChronoUnit, validUnits) {
  EXPECT_EQ(subsecond(1000000000), doLookup("ns"));
  EXPECT_EQ(subsecond(1000000000), doLookup("nanosecond"));
  EXPECT_EQ(subsecond(1000000000), doLookup("nanoseconds"));
  EXPECT_EQ(subsecond(1000000), doLookup("us"));
  EXPECT_EQ(subsecond(1000000), doLookup(u8"\u03BCs"));
  EXPECT_EQ(subsecond(1000000), doLookup("microsecond"));
  EXPECT_EQ(subsecond(1000000), doLookup("microseconds"));
  EXPECT_EQ(subsecond(1000), doLookup("ms"));
  EXPECT_EQ(subsecond(1000), doLookup("millisecond"));
  EXPECT_EQ(subsecond(1000), doLookup("milliseconds"));
  EXPECT_EQ(multisecond(1), doLookup("s"));
  EXPECT_EQ(multisecond(1), doLookup("seconds"));
  EXPECT_EQ(multisecond(1), doLookup("seconds"));
  EXPECT_EQ(multisecond(60), doLookup("m"));
  EXPECT_EQ(multisecond(60), doLookup("min"));
  EXPECT_EQ(multisecond(60), doLookup("minute"));
  EXPECT_EQ(multisecond(60), doLookup("minutes"));
  EXPECT_EQ(multisecond(3600), doLookup("h"));
  EXPECT_EQ(multisecond(3600), doLookup("hr"));
  EXPECT_EQ(multisecond(3600), doLookup("hour"));
  EXPECT_EQ(multisecond(3600), doLookup("hours"));
  EXPECT_EQ(multisecond(86400), doLookup("d"));
  EXPECT_EQ(multisecond(86400), doLookup("day"));
  EXPECT_EQ(multisecond(86400), doLookup("days"));
  EXPECT_EQ(multisecond(604800), doLookup("wk"));
  EXPECT_EQ(multisecond(604800), doLookup("week"));
  EXPECT_EQ(multisecond(604800), doLookup("weeks"));
  EXPECT_EQ(multisecond(2629746), doLookup("mon"));
  EXPECT_EQ(multisecond(2629746), doLookup("month"));
  EXPECT_EQ(multisecond(2629746), doLookup("months"));
  EXPECT_EQ(multisecond(31556952), doLookup("yr"));
  EXPECT_EQ(multisecond(31556952), doLookup("year"));
  EXPECT_EQ(multisecond(31556952), doLookup("years"));
}

TEST(ChronoUnit, invalidUnits) {
  using facebook::eden::lookupChronoUnitInfo;
  using namespace folly::literals::string_piece_literals;

  EXPECT_EQ(nullptr, lookupChronoUnitInfo(""));
  EXPECT_EQ(nullptr, lookupChronoUnitInfo("bogus"));
  EXPECT_EQ(nullptr, lookupChronoUnitInfo("nanosec"));
  EXPECT_EQ(nullptr, lookupChronoUnitInfo("nanoseconds2"));
  EXPECT_EQ(nullptr, lookupChronoUnitInfo("nanoseconds "));
  EXPECT_EQ(nullptr, lookupChronoUnitInfo("nanoseconds\0"_sp));
  EXPECT_EQ(nullptr, lookupChronoUnitInfo("minus"));
}
