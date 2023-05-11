/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Match.h"
#include <folly/portability/GTest.h>

namespace {

using namespace facebook::eden;

struct Thing {};

TEST(MatchTest, pattern_matches) {
  std::variant<int, std::string, Thing> v;

  v = 10;
  match(
      v,
      [](int i) { EXPECT_EQ(10, i); },
      [](const std::string&) { FAIL(); },
      [](const Thing&) { FAIL(); });

  v = "hello";
  match(
      v,
      [](int) { FAIL(); },
      [](const std::string& s) { EXPECT_EQ("hello", s); },
      [](const Thing&) { FAIL(); });

  // Does not compile, non-exhaustive:
  // Compiler errors are horrible though.
#if 0
  match(
      v,
      [](int) {});
#endif
}

TEST(MatchTest, const_variant) {
  const std::variant<int, float> v = 30.0f;
  match(
      v,
      [](const int&) { FAIL(); },
      [](const float& f) { EXPECT_EQ(30.0f, f); });
}

TEST(MatchTest, return_value) {
  std::variant<int, std::string> v;

  auto do_match = [&] {
    return match(
        v,
        [](const int& i) -> size_t { return i; },
        [](const std::string& s) { return s.size(); });
  };

  v = 10;
  EXPECT_EQ(10, do_match());

  v = "hello";
  EXPECT_EQ(5, do_match());
}

} // namespace
